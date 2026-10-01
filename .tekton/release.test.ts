import { describe, expect, test } from "bun:test";
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const source = readFileSync(join(import.meta.dir, "homelab-mcp-release.yaml"), "utf8");
const pipeline = Bun.YAML.parse(source) as any;
const tasks = pipeline.spec.pipelineSpec.tasks;
const task = (name: string) => tasks.find((entry: any) => entry.name === name);
const script = (name: string) => task(name).taskSpec.steps.find((step: any) => step.name !== "stage-crane").script as string;
const revision = "a".repeat(40);
const digest = `sha256:${"b".repeat(64)}`;
const image = `cr.holdenitdown.net/rfhold/homelab-mcp@${digest}`;

function configFixture(scenario: string, arch: string) {
  const config: any = { os: "linux", architecture: arch, config: { User: "65532:65532", Labels: { "org.opencontainers.image.revision": revision } } };
  if (scenario.startsWith("nested-")) config.unrelated = structuredClone(config);
  const defect = scenario.replace(/^nested-/, "");
  if (defect === "wrong-os") config.os = "windows";
  if (defect === "wrong-architecture") config.architecture = "wrong";
  if (defect === "wrong-user") config.config.User = "root";
  if (defect === "wrong-revision") config.config.Labels["org.opencontainers.image.revision"] = "wrong";
  const json = JSON.stringify(config);
  return json + (scenario === "malformed-config" ? " invalid" : scenario === "multiple-config" ? json : "");
}

function manifestFixture(scenario: string) {
  const manifest: any = { mediaType: "application/vnd.oci.image.index.v1+json", manifests: ["amd64", "arm64"].map((architecture) => ({ platform: { architecture, os: "linux" } })) };
  if (scenario === "single-manifest" || scenario === "nested-media-type") {
    manifest.unrelated = structuredClone(manifest);
    manifest.mediaType = "application/vnd.oci.image.manifest.v1+json";
  }
  if (scenario === "missing-arm64") manifest.manifests.pop();
  if (scenario === "nested-platform") {
    manifest.unrelated = structuredClone(manifest);
    manifest.manifests = [];
  }
  if (scenario === "wrong-platform-os") for (const entry of manifest.manifests) entry.platform.os = "windows";
  if (scenario === "manifest-not-array") manifest.manifests = { platform: { os: "linux", architecture: "amd64" } };
  const json = JSON.stringify(manifest);
  return json + (scenario === "malformed-manifest" ? " invalid" : scenario === "multiple-manifest" ? json : "");
}

function promote(scenario: string, overrides: Record<string, string> = {}) {
  const dir = mkdtempSync(join(tmpdir(), "homelab-mcp-release-test-"));
  try {
    const mock = join(dir, "crane");
    writeFileSync(mock, `#!/bin/sh
set -eu
printf '%s\\n' "$*" >> "$CALLS"
case "$1" in
  version) [ "$SCENARIO" != crane-unavailable ] || exit 1; printf 'fixture crane\n' ;;
  digest)
    case "$2" in
      *:preview-*)
        case "$SCENARIO" in
          missing-preview) printf 'MANIFEST_UNKNOWN\\n' >&2; exit 1 ;;
          bad-digest) printf 'not-a-digest\\n'; exit 0 ;;
        esac ;;
      *:v*)
        if [ ! -f "$COPIED" ]; then
          case "$SCENARIO" in
            existing-same) ;;
            existing-different) printf 'sha256:%064d\\n' 0; exit 0 ;;
            auth) printf 'UNAUTHORIZED\\n' >&2; exit 1 ;;
            network) printf 'connection timed out\\n' >&2; exit 1 ;;
            generic-404) printf '404 Not Found\\n' >&2; exit 1 ;;
            *) printf 'MANIFEST_UNKNOWN\\n' >&2; exit 1 ;;
          esac
        fi
        if [ "$SCENARIO" = changed-after-copy ]; then
          printf 'sha256:%064d\\n' 0; exit 0
        fi ;;
    esac
    printf '%s\\n' "$DIGEST" ;;
  manifest)
    printf '%s' "$MANIFEST_JSON" ;;
  config)
    case "$3" in
      linux/amd64) printf '%s' "$CONFIG_AMD64" ;;
      linux/arm64) printf '%s' "$CONFIG_ARM64" ;;
      *) exit 1 ;;
    esac ;;
  copy)
    [ "$SCENARIO" != copy-failure ] || exit 1
    touch "$COPIED" ;;
  *) exit 1 ;;
esac
`);
    chmodSync(mock, 0o755);
    const output = join(dir, "image");
    const calls = join(dir, "calls");
    writeFileSync(calls, "");
    const result = spawnSync("/bin/sh", ["-c", script("promote-release").replaceAll("$(results.image.path)", output)], {
      env: { ...process.env, PATH: `${dir}:${process.env.PATH}`, REVISION: revision, RELEASE_TAG: "v0.1.0", DIGEST: digest, SCENARIO: scenario, CALLS: calls, COPIED: join(dir, "copied"), MANIFEST_JSON: manifestFixture(scenario), CONFIG_AMD64: configFixture(scenario, "amd64"), CONFIG_ARM64: configFixture(scenario, "arm64"), ...overrides },
      encoding: "utf8",
    });
    return { status: result.status, calls: readFileSync(calls, "utf8"), image: existsSync(output) ? readFileSync(output, "utf8") : undefined };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

function validate(scenario: string, version = "0.1.0", branch = "refs/tags/v0.1.0") {
  const dir = mkdtempSync(join(tmpdir(), "homelab-mcp-release-policy-test-"));
  try {
    writeFileSync(join(dir, "Cargo.toml"), `[package]\nname = "homelab-mcp"\nversion = "${version}"\n`);
    writeFileSync(join(dir, "git"), `#!/bin/sh
set -eu
case "$1" in
  cat-file)
    if [ "$SCENARIO" = lightweight ]; then printf 'commit'; else printf 'tag'; fi ;;
  -c)
    case "$SCENARIO" in unsigned|untrusted) exit 1 ;; esac ;;
  rev-parse)
    case "$SCENARIO:$2" in
      sha-mismatch:refs/tags/*|head-mismatch:HEAD) printf '%040d' 0 ;;
      *) printf '%s' "$REVISION" ;;
    esac ;;
  merge-base) [ "$SCENARIO" != off-main ] ;;
  *) exit 1 ;;
esac
`);
    writeFileSync(join(dir, "gpg"), "#!/bin/sh\n[ \"$SCENARIO\" != missing-signers ]\n");
    chmodSync(join(dir, "git"), 0o755);
    chmodSync(join(dir, "gpg"), 0o755);
    const result = spawnSync("/bin/sh", ["-c", script("validate-release")
      .replace("/tmp/release-gnupg", join(dir, "gnupg"))
      .replaceAll("$(results.tag.path)", join(dir, "tag"))], {
      cwd: dir,
      env: { ...process.env, PATH: `${dir}:${process.env.PATH}`, REVISION: revision, SOURCE_BRANCH: branch, SCENARIO: scenario },
      encoding: "utf8",
    });
    return result.status;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

describe("signed stable release pipeline", () => {
  test("stages the pinned crane binary for the pinned jq image on amd64", () => {
    const promotion = task("promote-release");
    expect(promotion.taskSpec.volumes).toEqual([{ name: "crane-tool", emptyDir: {} }]);
    const [stage, verify] = promotion.taskSpec.steps;
    expect(stage.name).toBe("stage-crane");
    expect(stage.image).toBe("gcr.io/go-containerregistry/crane:debug@sha256:54b27703e6c602fbd6f95712910e9c8d45d4361a59274bde38aeec943734e424");
    expect(stage.script).toContain("cp /ko-app/crane /tools/crane");
    expect(stage.volumeMounts).toEqual([{ name: "crane-tool", mountPath: "/tools" }]);
    expect(verify.image).toBe("cr.holdenitdown.net/rfhold/general-ci@sha256:6943f91d774980357b24730a14ac4b026325d50962df4b2e15e24c3f43190e5b");
    expect(verify.volumeMounts).toEqual([{ name: "crane-tool", mountPath: "/tools", readOnly: true }]);
    expect(verify.script).toContain('export PATH="/tools:$PATH"');
    expect(verify.script).toContain("crane version");
    expect(pipeline.spec.taskRunSpecs.find((entry: any) => entry.pipelineTaskName === "promote-release").podTemplate.nodeSelector).toEqual({ "kubernetes.io/arch": "amd64" });
  });
  test("all embedded shell scripts parse", () => {
    for (const entry of tasks) {
      for (const step of entry.taskSpec.steps) {
        expect(spawnSync("/bin/sh", ["-n"], { input: step.script }).status).toBe(0);
      }
    }
  });

  test("has only fetch, policy, scan, digest promotion and production apply", () => {
    expect(tasks.map((entry: any) => entry.name)).toEqual(["fetch-repository", "validate-release", "scan-private-material", "promote-release", "deploy-production"]);
    expect(source).not.toMatch(/buildctl|buildkit|manifest-tool|docker build|cargo build|:latest.*homelab-mcp/);
    expect(pipeline.metadata.annotations["pipelinesascode.tekton.dev/on-cel-expression"]).toContain('event == "push"');
    expect(pipeline.metadata.annotations["pipelinesascode.tekton.dev/on-cel-expression"]).toContain("^refs/tags/v(0|[1-9][0-9]*)");
    expect(task("promote-release").runAfter).toEqual(["validate-release", "scan-private-material"]);
    expect(task("deploy-production").runAfter).toEqual(["promote-release"]);
  });

  test("requires trusted annotated signature, exact SHA, main ancestry and root package version", () => {
    const policy = script("validate-release");
    expect(policy).toContain('git cat-file -t "refs/tags/$tag")" = tag');
    expect(policy).toContain("/etc/release-signers/signing-key.asc");
    expect(policy).toContain("git -c gpg.format=openpgp -c gpg.program=gpg verify-tag");
    expect(policy).toContain('git rev-parse "refs/tags/$tag^{commit}")" = "$REVISION"');
    expect(policy).toContain('git merge-base --is-ancestor "$REVISION" refs/remotes/origin/main');
    expect(policy).toContain('in_package = ($0 == "[package]")');
    expect(policy).toContain("count != 1");
    expect(task("validate-release").taskSpec.volumes[0].secret.secretName).toBe("homelab-mcp-release-trusted-signers");
    expect(script("scan-private-material")).toContain('test "$status" -eq 1');
  });

  test("accepts matching root package version with all policy checks passing", () => {
    expect(validate("valid")).toBe(0);
  });

  for (const scenario of ["lightweight", "unsigned", "untrusted", "sha-mismatch", "head-mismatch", "off-main", "missing-signers"]) {
    test(`halts policy validation for ${scenario}`, () => {
      expect(validate(scenario)).not.toBe(0);
    });
  }

  test("rejects a Cargo package version mismatch and prerelease tag", () => {
    expect(validate("valid", "0.2.0")).not.toBe(0);
    expect(validate("valid", "0.1.0", "refs/tags/v0.1.0-rc.1")).not.toBe(0);
  });

  test("uses the existing bounded apply and credential boundary without OpenBao privileges", () => {
    const deploy = task("deploy-production");
    expect(deploy.taskSpec.steps[0].image).toBe("cr.holdenitdown.net/rfhold/general-ci@sha256:6943f91d774980357b24730a14ac4b026325d50962df4b2e15e24c3f43190e5b");
    expect(script("deploy-production")).toContain('test "$(bun --version)" = "1.3.5"');
    expect(script("deploy-production")).toContain('test "$(pulumi version)" = "v3.253.0"');
    expect(script("deploy-production")).toContain('pulumi up --stack prod --yes --skip-preview --config "image=$APP_IMAGE"');
    expect(source).not.toMatch(/pulumi preview|openbao-pulumi|openbao-login|VAULT_TOKEN|serviceAccountName:/);
    expect(deploy.params[0].value).toBe("$(tasks.promote-release.results.image)");
    expect(deploy.taskSpec.volumes[0].secret.secretName).toBe("tekton-cluster-kubeconfig");
    expect(deploy.taskSpec.steps[0].envFrom.map((entry: any) => entry.secretRef.name)).toEqual(["pulumi-credentials", "authentik-credentials"]);
    expect(deploy.taskSpec.steps[0].env.filter((entry: any) => entry.valueFrom).map((entry: any) => entry.valueFrom.secretKeyRef.key)).toEqual(["GRAFANA_URL", "GRAFANA_TOKEN"]);
  });

  test("copies only the immutable preview index and verifies both Linux architectures", () => {
    const result = promote("absent");
    expect(result.status).toBe(0);
    expect(result.image).toBe(image);
    expect(result.calls).toContain(`copy ${image} cr.holdenitdown.net/rfhold/homelab-mcp:v0.1.0`);
    expect(result.calls).toContain(`config --platform linux/amd64 ${image}`);
    expect(result.calls).toContain(`config --platform linux/arm64 ${image}`);
  });

  test("an identical stable alias is idempotent and not overwritten", () => {
    const result = promote("existing-same");
    expect(result.status).toBe(0);
    expect(result.image).toBe(image);
    expect(result.calls).not.toContain("copy ");
  });

  for (const scenario of ["crane-unavailable", "missing-preview", "bad-digest", "single-manifest", "missing-arm64", "wrong-user", "wrong-revision", "wrong-os", "wrong-architecture", "nested-wrong-user", "nested-wrong-revision", "nested-wrong-os", "nested-wrong-architecture", "malformed-config", "multiple-config", "nested-platform", "nested-media-type", "wrong-platform-os", "manifest-not-array", "malformed-manifest", "multiple-manifest", "existing-different", "auth", "network", "generic-404"]) {
    test(`fails without any copy for ${scenario}`, () => {
      const result = promote(scenario);
      expect(result.status).not.toBe(0);
      expect(result.calls).not.toContain("copy ");
      expect(result.image).toBeUndefined();
    });
  }

  for (const scenario of ["copy-failure", "changed-after-copy"]) {
    test(`does not emit a deployable image for ${scenario}`, () => {
      const result = promote(scenario);
      expect(result.status).not.toBe(0);
      expect(result.image).toBeUndefined();
    });
  }

  for (const tag of ["v01.1.0", "v0.1.0-rc.1", "v0.1", "main", "v0.1.0;true", "v0.1.0\nmalicious"]) {
    test(`rejects non-stable tag ${tag} before registry access`, () => {
      const result = promote("absent", { RELEASE_TAG: tag });
      expect(result.status).not.toBe(0);
      expect(result.calls).toBe("");
    });
  }
});
