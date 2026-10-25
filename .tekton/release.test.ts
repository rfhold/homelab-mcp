import { describe, expect, test } from "bun:test";
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const source = readFileSync(join(import.meta.dir, "homelab-mcp-release.yaml"), "utf8");
const pipeline = Bun.YAML.parse(source) as any;
const tasks = pipeline.spec.pipelineSpec.tasks;
const task = (name: string) => tasks.find((entry: any) => entry.name === name);
const script = (name: string) => task(name).taskSpec.steps[0].script as string;
const revision = "a".repeat(40);

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
  const manifest: any = { schemaVersion: 2, mediaType: "application/vnd.oci.image.index.v1+json", manifests: ["amd64", "arm64"].map((architecture) => descriptor(scenario, architecture)) };
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

const sharedTasks = process.env.SHARED_TASKS_DIR ?? join(import.meta.dir, "../../homelab/src/components/tekton-tasks");
const sharedPromotion = (Bun.YAML.parse(readFileSync(join(sharedTasks, "service-promote-v1.yaml"), "utf8")) as any).spec.steps[0].script;
function hash(text: string) { return `sha256:${new Bun.CryptoHasher("sha256").update(text).digest("hex")}`; }
const configType = "application/vnd.oci.image.config.v1+json";
const manifestType = "application/vnd.oci.image.manifest.v1+json";
function platformFixture(scenario: string, arch: string) {
  const config = configFixture(scenario, arch);
  return JSON.stringify({ schemaVersion: 2, mediaType: manifestType, config: { mediaType: configType, digest: hash(config), size: Buffer.byteLength(config) }, layers: [] });
}
function descriptor(scenario: string, architecture: string) {
  const manifest = platformFixture(scenario, architecture);
  return { mediaType: manifestType, digest: hash(manifest), size: Buffer.byteLength(manifest), platform: { architecture, os: "linux" } };
}

const digest = hash(manifestFixture("absent"));
const image = `cr.holdenitdown.net/rfhold/homelab-mcp@${digest}`;

function promote(scenario: string, overrides: Record<string, string> = {}) {
  const directory = mkdtempSync(join(tmpdir(), "shared-release-test-"));
  try {
    const output = join(directory, "images");
    const calls = join(directory, "calls");
    const scriptPath = join(directory, "promote.ts");
    const repository = JSON.parse(task("promote-release").params.find((p: any) => p.name === "images").value)[0].repository;
    const objects: Record<string, string> = {};
    for (const arch of ["amd64", "arm64"]) {
      const config = configFixture(scenario, arch), manifest = platformFixture(scenario, arch);
      objects[`blobs/${hash(config)}`] = config;
      objects[`manifests/${hash(manifest)}`] = manifest;
    }
    const index = manifestFixture(scenario);
    objects[`manifests/${revision}`] = index;
    const harness = `
import { appendFileSync } from "node:fs";
const objects = ${JSON.stringify(objects)};
const index = ${JSON.stringify(index)};
let copied = false;
globalThis.fetch = async (url, options = {}) => {
  const key = String(url).split("/v2/")[1].split("/").slice(2).join("/");
  const method = options.method || "GET";
  appendFileSync(process.env.CALLS, method + " " + key + "\\n");
  const scenario = process.env.SCENARIO;
  const response = (body, status = 200, headers = {}) => new Response(body, { status, headers });
  const missing = () => response(JSON.stringify({ errors: [{ code: "MANIFEST_UNKNOWN" }] }), 404);
  if (method === "PUT") {
    if (scenario === "copy-failure") return response("failed", 500);
    copied = true; return response("", 201);
  }
  if (key === "manifests/" + process.env.REVISION) {
    if (["missing-preview", "preview-missing"].includes(scenario)) return missing();
    if (scenario === "transport-unavailable") throw Error("unavailable");
    if (["bad-digest", "malformed"].includes(scenario)) return response(index, 200, { "docker-content-digest": "sha256:bad" });
  }
  if (key.startsWith("manifests/v")) {
    if (copied && scenario === "changed-after-copy") return response(index + " ", 200);
    if (["existing-different", "conflict"].includes(scenario)) return response(index + " ", 200);
    if (["auth", "unauthorized"].includes(scenario)) return response("unauthorized", 403);
    if (["network", "timeout"].includes(scenario)) throw Error("timeout");
    if (["generic-404", "notfound"].includes(scenario)) return response("404 Not Found", 404);
    if (scenario === "nameunknown") return response(JSON.stringify({ errors: [{ code: "NAME_UNKNOWN" }] }), 404);
    if (copied || ["existing-same", "same"].includes(scenario)) return response(index);
    return missing();
  }
  return objects[key] === undefined ? missing() : response(objects[key]);
};
`;
    writeFileSync(scriptPath, harness + sharedPromotion.replace(/^#!.*\n/, ""));
    writeFileSync(calls, "");
    const result = Bun.spawnSync([process.execPath, scriptPath], {
      env: { ...process.env, DOCKER_CONFIG: directory, REVISION: revision, VERSION: overrides.RELEASE_TAG ?? "v0.1.0", IMAGES: JSON.stringify([{ key: "image", repository, platforms: ["amd64", "arm64"], user: "65532:65532" }]), IMAGES_RESULT: output, SCENARIO: scenario, CALLS: calls },
    });
    const value = existsSync(output) ? JSON.parse(readFileSync(output, "utf8")).image : undefined;
    return { status: result.exitCode, error: result.stderr.toString(), calls: readFileSync(calls, "utf8"), image: value, result: value ?? "" };
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

describe("signed stable release pipeline", () => {
  test("delegates metadata verification and promotion to the installed shared Task", () => {
    const promotion = task("promote-release");
    expect((promotion as any).taskRef).toEqual({ name: "service-promote-v1" });
    expect(promotion.taskSpec).toBeUndefined();
    expect(promotion.params).toEqual([
      { name: "revision", value: "$(params.revision)" },
      { name: "version", value: "$(tasks.validate-release.results.tag)" },
      { name: "images", value: JSON.stringify([{ key: "image", repository: "cr.holdenitdown.net/rfhold/homelab-mcp", platforms: ["amd64", "arm64"], user: "65532:65532" }]) },
    ]);
  });
  test("all embedded shell scripts parse", () => {
    for (const entry of tasks) {
      for (const step of (entry.taskSpec?.steps ?? [])) {
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
    expect((deploy as any).taskRef).toEqual({ name: "service-pulumi-up-v1" });
    expect(deploy.taskSpec).toBeUndefined();
    expect(Object.fromEntries(deploy.params.map((param: any) => [param.name, param.value]))).toEqual({
      "source-subdir": "infra/pulumi", stack: "prod", "image-config": "$(tasks.promote-release.results.images)",
      "authentik-enabled": "true", "grafana-enabled": "true", "openbao-enabled": "false",
    });
    expect(source).not.toMatch(/pulumi preview|openbao-pulumi|openbao-login|VAULT_TOKEN|serviceAccountName:/);
  });

  test("copies only the immutable preview index and verifies both Linux architectures", () => {
    const result = promote("absent");
    expect(result.status, result.error).toBe(0);
    expect(result.image).toBe(image);
    expect(result.calls).toContain(`PUT manifests/v0.1.0`);
    expect(result.calls).toContain(`GET manifests/${descriptor("absent", "amd64").digest}`);
    expect(result.calls).toContain(`GET manifests/${descriptor("absent", "arm64").digest}`);
  });

  test("an identical stable alias is idempotent and not overwritten", () => {
    const result = promote("existing-same");
    expect(result.status, result.error).toBe(0);
    expect(result.image).toBe(image);
    expect(result.calls).not.toContain("PUT ");
  });

  for (const scenario of ["transport-unavailable", "missing-preview", "bad-digest", "single-manifest", "missing-arm64", "wrong-user", "wrong-revision", "wrong-os", "wrong-architecture", "nested-wrong-user", "nested-wrong-revision", "nested-wrong-os", "nested-wrong-architecture", "malformed-config", "multiple-config", "nested-platform", "nested-media-type", "wrong-platform-os", "manifest-not-array", "malformed-manifest", "multiple-manifest", "existing-different", "auth", "network", "generic-404"]) {
    test(`fails without any copy for ${scenario}`, () => {
      const result = promote(scenario);
      expect(result.status).not.toBe(0);
      expect(result.calls).not.toContain("PUT ");
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
