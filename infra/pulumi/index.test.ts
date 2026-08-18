import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { after, before, describe, test } from "node:test";
import * as pulumi from "@pulumi/pulumi";
import {
  allConfig,
  setAllConfig,
} from "@pulumi/pulumi/runtime/config";
import {
  openBaoHttpsEgressRules,
  requireImmutableImage,
  validateCephClusters,
  validateCidrs,
  validateHttpsOrigin,
  validateKubernetesClusters,
  validateOpenBaoSegment,
  validateOpenBaoStack,
  validatePort,
  validateWrappingKeyVersions,
} from "./policy";

interface ResourceRecord {
  type: string;
  name: string;
  inputs: Record<string, unknown>;
  provider?: string;
}

const resources: ResourceRecord[] = [];
const calls: ResourceRecord[] = [];
const previousConfig = process.env.PULUMI_CONFIG;
const previousGrafanaUrl = process.env.GRAFANA_URL;
const previousGrafanaAuth = process.env.GRAFANA_AUTH;
const previousForgejoToken = process.env.FORGEJO_HOLDENITDOWN_TOKEN;
const previousPacIncomingSecret = process.env.PAC_INCOMING_SECRET;
const previousCephRomulusUsername = process.env.CEPH_DASHBOARD_ROMULUS_USERNAME;
const previousCephRomulusPassword = process.env.CEPH_DASHBOARD_ROMULUS_PASSWORD;
const previousCephPantheonUsername = process.env.CEPH_DASHBOARD_PANTHEON_USERNAME;
const previousCephPantheonPassword = process.env.CEPH_DASHBOARD_PANTHEON_PASSWORD;
const forgejoTokenFixture = "test-forgejo-token";
const pacIncomingSecretFixture = "test-pac-incoming-secret";
const cephUsernameFixture = "test-ceph-user";
const cephPasswordFixture = "test-ceph-password";
let program: typeof import("./index");

before(async () => {
  process.env.PULUMI_CONFIG = JSON.stringify({
    "homelab-mcp:namespace": "homelab-mcp-test",
    "homelab-mcp:hostname": "homelab-mcp.example.test",
    "homelab-mcp:displayName": "Homelab MCP Test",
    "homelab-mcp:slug": "homelab-mcp-test",
    "homelab-mcp:image":
      "registry.example.test/homelab-mcp@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "homelab-mcp:authentikBaseUrl": "https://auth.example.test",
    "homelab-mcp:protectData": "true",
    "homelab-mcp:storageClass": "test-storage",
    "homelab-mcp:databaseStorageSize": "2Gi",
    "homelab-mcp:backupStorageClass": "test-bucket",
    "homelab-mcp:backupEndpoint": "https://s3.example.test",
    "homelab-mcp:kubernetesClusters": JSON.stringify([
      {
        name: "pantheon",
        context: "pantheon",
        server: "https://pantheon.example.test:6443",
        apiServerEndpointCidrs: ["172.16.3.0/24"],
      },
      {
        name: "romulus",
        context: "romulus",
        server: "https://romulus.example.test",
        apiServerEndpointCidrs: ["172.16.4.0/24"],
      },
    ]),
    "homelab-mcp:cephClusters": JSON.stringify([
      {
        name: "romulus",
        origin: "https://ceph.romulus.example.test",
        expectedMajorRelease: 19,
      },
      {
        name: "pantheon",
        origin: "https://ceph.pantheon.example.test",
        expectedMajorRelease: 19,
      },
    ]),
    "homelab-mcp:backupRetention": "7d",
    "homelab-mcp:backupSchedule": "0 30 1 * * *",
    "homelab-mcp:mcpOAuthAccessTokenTtl": "300",
    "homelab-mcp:mcpOAuthRefreshTokenTtl": "86400",
    "homelab-mcp:mcpOAuthRefreshFamilyTtl": "2592000",
    "homelab-mcp:mcpOAuthCodeTtl": "300",
    "homelab-mcp:mcpOAuthCimdTrustedPrivateOrigins":
      '["https://kuri.internal.example"]',
    "homelab-mcp:mcpOAuthWrappingKeyVersions": '["v1"]',
    "homelab-mcp:mcpOAuthActiveWrappingKeyVersion": "v1",
    "homelab-mcp:openbaoEnabled": "true",
    "homelab-mcp:openbaoUrl": "https://openbao.example.test",
    "homelab-mcp:openbaoCreateSshMount": "true",
    "homelab-mcp:openbaoKubernetesAuthMount": "kubernetes",
    "homelab-mcp:openbaoKubernetesRole": "homelab-mcp-test",
    "homelab-mcp:openbaoSshMount": "homelab-ssh-client",
    "homelab-mcp:openbaoSshRole": "homelab",
    "homelab-mcp:openbaoAudience": "https://openbao.example.test",
    "homelab-mcp:openbaoEndpointCidrs": '["172.16.5.10/32"]',
    "homelab-mcp:openbaoPort": "8200",
    "homelab-mcp:deployMachineSshEgressCidrs": '["172.16.0.0/16"]',
  });
  process.env.GRAFANA_URL = "https://grafana.example.test";
  process.env.GRAFANA_AUTH = "bootstrap:test-password";
  process.env.FORGEJO_HOLDENITDOWN_TOKEN = forgejoTokenFixture;
  process.env.PAC_INCOMING_SECRET = pacIncomingSecretFixture;
  process.env.CEPH_DASHBOARD_ROMULUS_USERNAME = cephUsernameFixture;
  process.env.CEPH_DASHBOARD_ROMULUS_PASSWORD = cephPasswordFixture;
  process.env.CEPH_DASHBOARD_PANTHEON_USERNAME = cephUsernameFixture;
  process.env.CEPH_DASHBOARD_PANTHEON_PASSWORD = cephPasswordFixture;

  pulumi.runtime.setMocks(
    {
      newResource: (args) => {
        resources.push({
          type: args.type,
          name: args.name,
          inputs: args.inputs,
          provider: args.provider,
        });
        const outputs: Record<string, unknown> = { ...args.inputs };
        if (args.type === "random:index/randomBytes:RandomBytes") {
          outputs.base64 = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        }
        if (args.type === "random:index/randomPassword:RandomPassword") {
          outputs.result = "test-client-secret";
        }
        if (args.type === "tls:index/privateKey:PrivateKey") {
          outputs.privateKeyPem = "test-private-key";
        }
        if (args.type === "tls:index/selfSignedCert:SelfSignedCert") {
          outputs.certPem = "test-certificate";
        }
        if (
          args.type === "authentik:index/certificateKeyPair:CertificateKeyPair"
        ) {
          outputs.id = "signing-key";
        }
        if (args.type === "grafana:oss/serviceAccountToken:ServiceAccountToken") {
          outputs.key = "test-grafana-token";
        }
        if (args.type === "pulumi:index:Stash") {
          outputs.output = args.inputs.input;
        }
        if (args.name === "homelab-mcp-backups-generated-config") {
          outputs.data = { BUCKET_NAME: "test-backup-bucket" };
        }
        if (args.name === "homelab-mcp-postgres-generated-app") {
          outputs.data = {
            username: Buffer.from("app").toString("base64"),
            password: Buffer.from("password").toString("base64"),
          };
        }
        if (args.name.startsWith("homelab-mcp-kubernetes-runtime-token-read-")) {
          outputs.data = {
            token: Buffer.from("synthetic-runtime-token").toString("base64"),
            "ca.crt": Buffer.from("synthetic-cluster-ca").toString("base64"),
          };
        }
        return { id: `${args.name}-id`, state: outputs };
      },
      call: (args) => {
        calls.push({ type: args.token, name: args.token, inputs: args.inputs });
        return {
          ...args.inputs,
          id: "lookup-id",
          propertyMappingProviderScopeId: "mapping-id",
          data: {
            BUCKET_NAME: "test-backup-bucket",
            username: Buffer.from("app").toString("base64"),
            password: Buffer.from("password").toString("base64"),
          },
        };
      },
    },
    "homelab-mcp",
    "test",
    false,
  );
  program = await import("./index");
  await pulumi.runtime.disconnect();
});

after(() => {
  restoreEnv("PULUMI_CONFIG", previousConfig);
  restoreEnv("GRAFANA_URL", previousGrafanaUrl);
  restoreEnv("GRAFANA_AUTH", previousGrafanaAuth);
  restoreEnv("FORGEJO_HOLDENITDOWN_TOKEN", previousForgejoToken);
  restoreEnv("PAC_INCOMING_SECRET", previousPacIncomingSecret);
  restoreEnv("CEPH_DASHBOARD_ROMULUS_USERNAME", previousCephRomulusUsername);
  restoreEnv("CEPH_DASHBOARD_ROMULUS_PASSWORD", previousCephRomulusPassword);
  restoreEnv("CEPH_DASHBOARD_PANTHEON_USERNAME", previousCephPantheonUsername);
  restoreEnv("CEPH_DASHBOARD_PANTHEON_PASSWORD", previousCephPantheonPassword);
});

describe("configuration policy", () => {
  test("accepts only immutable sha256 image references", () => {
    const digest = `registry.example.test/app@sha256:${"b".repeat(64)}`;
    assert.equal(requireImmutableImage(digest), digest);
    assert.throws(() => requireImmutableImage("registry.example.test/app:main"));
    assert.throws(() => requireImmutableImage("registry.example.test/app@sha256:abc"));
  });

  test("validates HTTPS origins and versioned wrapping keys", () => {
    assert.equal(
      validateHttpsOrigin("https://auth.example.test/", "auth"),
      "https://auth.example.test",
    );
    assert.throws(() => validateHttpsOrigin("http://auth.example.test", "auth"));
    assert.throws(() => validateHttpsOrigin("https://auth.example.test/path", "auth"));
    assert.doesNotThrow(() => validateWrappingKeyVersions(["v1"], "v1"));
    assert.throws(() => validateWrappingKeyVersions([], "v1"));
    assert.throws(() => validateWrappingKeyVersions(["v1", "v1"], "v1"));
    assert.throws(() => validateWrappingKeyVersions(["v1"], "v2"));
  });

  test("validates bounded OpenBao path and network configuration", () => {
    assert.equal(validateOpenBaoSegment("ssh-client", "mount"), "ssh-client");
    assert.throws(() => validateOpenBaoSegment("ssh/client", "mount"));
    assert.deepEqual(validateCidrs(["10.0.0.1/32", "10.0.0.0/24"], "cidrs"), ["10.0.0.1/32", "10.0.0.0/24"]);
    assert.throws(() => validateCidrs(["0.0.0.0/0x"], "cidrs"));
    assert.throws(() => validateCidrs(["fd00::/64"], "cidrs"));
    assert.equal(validatePort(8200, "port"), 8200);
    assert.throws(() => validatePort(0, "port"));
    assert.equal(validateOpenBaoStack("prod", false, false), false);
    assert.equal(validateOpenBaoStack("preview", true, true), true);
    assert.throws(() => validateOpenBaoStack("prod", true, false));
    assert.throws(() => validateOpenBaoStack("prod", false, true));
    assert.deepEqual(openBaoHttpsEgressRules(false, ["172.16.5.10/32"], 8200), [
      {
        to: [{ ipBlock: { cidr: "0.0.0.0/0" } }],
        ports: [{ port: 443, protocol: "TCP" }],
      },
    ]);
  });

  test("validates a bounded, unique, reviewed Kubernetes cluster catalog", () => {
    const valid = [
      {
        name: "pantheon",
        context: "pantheon",
        server: "https://pantheon.example.test:6443",
        apiServerEndpointCidrs: ["172.16.3.0/24"],
      },
    ];
    assert.deepEqual(validateKubernetesClusters(valid), [
      { ...valid[0], apiServerPort: 6443 },
    ]);
    assert.equal(
      validateKubernetesClusters([
        { ...valid[0], server: "https://cluster.example.test/" },
      ])[0].apiServerPort,
      443,
    );
    assert.throws(() => validateKubernetesClusters(null));
    assert.throws(() => validateKubernetesClusters({}));
    assert.throws(() => validateKubernetesClusters("not-an-array"));
    assert.throws(() => validateKubernetesClusters(pulumi.unknown));
    assert.throws(() => validateKubernetesClusters([]));
    assert.throws(() => validateKubernetesClusters(Array(33).fill(valid[0])));
    assert.throws(() => validateKubernetesClusters([...valid, valid[0]]));
    assert.throws(() =>
      validateKubernetesClusters([{ ...valid[0], name: "Unsafe_Name" }]),
    );
    assert.throws(() =>
      validateKubernetesClusters([{ ...valid[0], context: "" }]),
    );
    assert.throws(() =>
      validateKubernetesClusters([
        ...valid,
        { ...valid[0], name: "romulus" },
      ]),
    );
    assert.throws(() =>
      validateKubernetesClusters([{ ...valid[0], server: "http://cluster.test" }]),
    );
    assert.throws(() =>
      validateKubernetesClusters([{ ...valid[0], server: "https://cluster.test:0" }]),
    );
    assert.throws(() => validateKubernetesClusters([{ ...valid[0], name: 7 }]));
    assert.throws(() =>
      validateKubernetesClusters([
        {
          name: "pantheon",
          context: "pantheon",
          server: "https://pantheon.example.test:6443",
        },
      ]),
    );
    for (const extra of [
      { token: "credential" },
      { certificateAuthorityData: "credential" },
      { username: "credential", password: "credential" },
    ]) {
      assert.throws(() => validateKubernetesClusters([{ ...valid[0], ...extra }]));
    }
    assert.throws(() =>
      validateKubernetesClusters([
        { ...valid[0], apiServerEndpointCidrs: [] },
      ]),
    );
    assert.throws(() =>
      validateKubernetesClusters([
        { ...valid[0], apiServerEndpointCidrs: ["0.0.0.0/0x"] },
      ]),
    );
  });

  test("validates a bounded credential-free Ceph Dashboard catalog", () => {
    const valid = {
      name: "romulus",
      origin: "https://ceph.romulus.example.test/",
      expectedMajorRelease: 19,
    };
    assert.deepEqual(validateCephClusters([valid]), [
      { ...valid, origin: "https://ceph.romulus.example.test" },
    ]);
    assert.deepEqual(validateCephClusters([]), []);
    assert.throws(() => validateCephClusters(null));
    assert.throws(() => validateCephClusters(Array(33).fill(valid)));
    assert.throws(() => validateCephClusters([valid, valid]));
    assert.throws(() =>
      validateCephClusters([{ ...valid, origin: "http://ceph.example.test" }]),
    );
    assert.throws(() =>
      validateCephClusters([
        { ...valid, origin: "https://user:password@ceph.example.test" },
      ]),
    );
    assert.throws(() =>
      validateCephClusters([{ ...valid, expectedMajorRelease: 18 }]),
    );
    assert.throws(() => validateCephClusters([{ ...valid, name: "Unsafe_Name" }]));
    for (const extra of [
      { username: "embedded" },
      { password: "embedded" },
      { token: "embedded" },
    ]) {
      assert.throws(() => validateCephClusters([{ ...valid, ...extra }]));
    }
  });

  test("defines preview and production targets without images or secrets", () => {
    const preview = stackFile("preview");
    const production = stackFile("prod");
    assert.match(preview, /^\s*homelab-mcp:namespace: homelab-mcp-preview$/m);
    assert.match(
      preview,
      /^\s*homelab-mcp:hostname: preview-homelab-mcp\.holdenitdown\.net$/m,
    );
    assert.match(production, /^\s*homelab-mcp:namespace: homelab-mcp$/m);
    assert.match(
      production,
      /^\s*homelab-mcp:hostname: homelab-mcp\.holdenitdown\.net$/m,
    );
    assert.match(preview, /^\s*homelab-mcp:protectData: (?:"false"|false)$/m);
    assert.match(production, /^\s*homelab-mcp:protectData: (?:"true"|true)$/m);
    assert.match(production, /^\s*homelab-mcp:cephClusters: \[\]$/m);
    assert.match(preview, /^\s*homelab-mcp:openbaoEnabled: true$/m);
    assert.match(production, /^\s*homelab-mcp:openbaoEnabled: false$/m);
    assert.match(production, /^\s*homelab-mcp:openbaoCreateSshMount: false$/m);
    assert.match(
      preview,
      /^\s*homelab-mcp:deployMachineSshEgressCidrs: \[172\.16\.0\.0\/16\]$/m,
    );
    assert.match(
      production,
      /^\s*homelab-mcp:deployMachineSshEgressCidrs: \[\]$/m,
    );
    assert.doesNotMatch(production, /https:\/\/ceph\./);
    assert.match(preview, /^\s*homelab-mcp:cephClusters:$/m);
    const cephCatalog = preview.slice(
      preview.indexOf("  homelab-mcp:cephClusters:"),
      preview.indexOf("  homelab-mcp:mcpOAuthAccessTokenTtl:"),
    );
    assert.deepEqual(
      [...cephCatalog.matchAll(/^\s*- name: ([a-z0-9-]+)$/gm)].map(
        (match) => match[1],
      ),
      ["romulus", "pantheon"],
    );
    assert.match(cephCatalog, /https:\/\/ceph\.romulus\.holdenitdown\.net/);
    assert.match(cephCatalog, /https:\/\/ceph\.pantheon\.holdenitdown\.net/);
    assert.equal(
      (cephCatalog.match(/^\s*expectedMajorRelease: 19$/gm) ?? []).length,
      2,
    );
    for (const stack of [preview, production]) {
      assert.match(stack, /^\s*kubernetes:context: pantheon$/m);
      assert.doesNotMatch(stack, /^\s*homelab-mcp:image:/m);
      assert.doesNotMatch(
        stack,
        /(?:password|secret|privateKey|accessKeyId|GRAFANA_AUTH)\s*:/i,
      );
      assert.match(
        stack,
        /^\s*homelab-mcp:mcpOAuthWrappingKeyVersions: \[v1\]$/m,
      );
      assert.match(stack, /^\s*- name: pantheon$/m);
      assert.match(stack, /^\s*- name: romulus$/m);
      assert.match(stack, /https:\/\/pantheon\.holdenitdown\.net:6443/);
      assert.match(stack, /https:\/\/romulus\.holdenitdown\.net:6443/);
      assert.match(stack, /172\.16\.3\.0\/24/);
      assert.match(stack, /172\.16\.4\.0\/24/);
      assert.doesNotMatch(stack, /kubernetesApiEndpointCidr:/);
    }
  });
});

describe("standalone resource topology", () => {
  test("creates the namespace and protected CNPG database backups", () => {
    assert.equal(
      resource("kubernetes:core/v1:Namespace", "homelab-mcp-namespace").inputs
        .metadata != null,
      true,
    );
    const bucket = resourceByName("homelab-mcp-backups-bucket");
    assert.equal(bucket.inputs.kind, "ObjectBucketClaim");
    assert.equal((bucket.inputs.spec as any).storageClassName, "test-bucket");

    const cluster = resourceByName("homelab-mcp-postgres");
    assert.equal(cluster.inputs.kind, "Cluster");
    const clusterSpec = cluster.inputs.spec as any;
    assert.equal(clusterSpec.storage.storageClass, "test-storage");
    assert.equal(clusterSpec.storage.size, "2Gi");
    assert.equal(clusterSpec.backup.retentionPolicy, "7d");
    assert.equal(
      clusterSpec.backup.barmanObjectStore.destinationPath,
      "s3://test-backup-bucket/database",
    );
    assert.deepEqual(clusterSpec.backup.barmanObjectStore.s3Credentials, {
      accessKeyId: {
        name: "homelab-mcp-backups",
        key: "AWS_ACCESS_KEY_ID",
      },
      secretAccessKey: {
        name: "homelab-mcp-backups",
        key: "AWS_SECRET_ACCESS_KEY",
      },
    });

    const database = resourceByName("homelab-mcp-database").inputs;
    assert.equal(database.kind, "Database");
    assert.equal((database.spec as any).name, "homelab_mcp");
    const backup = resourceByName("homelab-mcp-postgres-backup").inputs;
    assert.equal(backup.kind, "ScheduledBackup");
    assert.equal((backup.spec as any).schedule, "0 30 1 * * *");
    assert.equal((backup.spec as any).method, "barmanObjectStore");
  });

  test("creates one strict confidential Authentik browser application", () => {
    const oauthProviders = resources.filter(
      (candidate) =>
        candidate.type === "authentik:index/providerOauth2:ProviderOauth2",
    );
    assert.equal(oauthProviders.length, 1);
    const provider = oauthProviders[0].inputs;
    assert.equal(provider.clientType, "confidential");
    assert.notEqual(provider.clientSecret, undefined);
    assert.deepEqual(provider.propertyMappings, [
      "lookup-id",
      "lookup-id",
      "lookup-id",
    ]);
    assert.deepEqual(
      calls
        .filter((call) =>
          String(call.inputs.managed).startsWith(
            "goauthentik.io/providers/oauth2/scope-",
          ),
        )
        .map((call) => call.inputs.managed)
        .sort(),
      [
        "goauthentik.io/providers/oauth2/scope-email",
        "goauthentik.io/providers/oauth2/scope-openid",
        "goauthentik.io/providers/oauth2/scope-profile",
      ],
    );
    assert.deepEqual(provider.allowedRedirectUris, [
      {
        matching_mode: "strict",
        url: "https://homelab-mcp.example.test/oidc/callback",
      },
    ]);
    assert.equal(
      resources.some((candidate) => candidate.name.includes("native")),
      false,
    );
  });

  test("bootstraps an Editor Grafana account with a non-rotating, non-expiring token", () => {
    const provider = resource(
      "pulumi:providers:grafana",
      "homelab-mcp-grafana",
    );
    assert.equal(provider.inputs.url, "https://grafana.example.test");
    assert.ok(isSecret(provider.inputs.auth));

    const account = resource(
      "grafana:oss/serviceAccount:ServiceAccount",
      "homelab-mcp-grafana-service-account",
    );
    assert.equal(account.inputs.role, "Editor");
    const token = resource(
      "grafana:oss/serviceAccountToken:ServiceAccountToken",
      "homelab-mcp-grafana-token",
    );
    assert.equal(token.inputs.secondsToLive, 0);
    assert.equal(
      resources.some((candidate) =>
        candidate.type.includes("ServiceAccountRotatingToken"),
      ),
      false,
    );
  });

  test("keeps runtime credentials and hosted OAuth settings in Secrets", () => {
    const app = environment();
    assert.match(app.HOMELAB_MCP_DATABASE_URL as string, /sslmode=verify-full/);
    assert.match(
      app.HOMELAB_MCP_DATABASE_URL as string,
      /sslrootcert=%2Fvar%2Frun%2Fsecrets%2Fhomelab-mcp%2Fpostgres%2Fca\.crt/,
    );
    assert.equal(
      app.HOMELAB_MCP_OIDC_ISSUER,
      "https://auth.example.test/application/o/homelab-mcp-test-browser/",
    );
    assert.equal(app.HOMELAB_MCP_OIDC_CLIENT_SECRET, "test-client-secret");
    assert.equal(app.HOMELAB_MCP_OIDC_SCOPES, "openid profile email");
    assert.equal(app.HOMELAB_MCP_OAUTH_ISSUER, "https://homelab-mcp.example.test/oauth");
    assert.equal(app.HOMELAB_MCP_OAUTH_RESOURCE, "https://homelab-mcp.example.test/mcp");
    assert.equal(
      app.HOMELAB_MCP_OAUTH_REQUIRED_SCOPES,
      "mcp:use kubernetes:read kubernetes:write inventory:read inventory:write inventory:host-trust deploy:read deploy:run",
    );
    assert.equal(app.HOMELAB_MCP_KUBECTL_PATH, "/usr/local/bin/kubectl");
    assert.equal(app.HOMELAB_MCP_DATABASE_MAX_CONNECTIONS, "10");
    assert.equal(app.HOMELAB_MCP_DEPLOY_ROOT, "/opt/homelab-mcp");
    assert.equal(app.HOMELAB_MCP_DEPLOY_CATALOG, "/opt/homelab-mcp/deploys/catalog.json");
    assert.equal(app.HOMELAB_MCP_DEPLOY_UV_EXECUTABLE, "/usr/local/bin/uv");
    assert.equal(app.HOMELAB_MCP_DEPLOY_SSH_KEYGEN_EXECUTABLE, "/usr/bin/ssh-keygen");
    assert.equal(app.HOMELAB_MCP_DEPLOY_TEMP_ROOT, "/var/run/homelab-mcp/deploy");
    assert.equal(app.HOMELAB_MCP_OPENBAO_URL, "https://openbao.example.test");
    assert.equal(app.HOMELAB_MCP_OPENBAO_KUBERNETES_AUTH_MOUNT, "kubernetes");
    assert.equal(app.HOMELAB_MCP_OPENBAO_KUBERNETES_ROLE, "homelab-mcp-test");
    assert.equal(app.HOMELAB_MCP_OPENBAO_SSH_MOUNT, "homelab-ssh-client");
    assert.equal(app.HOMELAB_MCP_OPENBAO_SSH_ROLE, "homelab");
    assert.equal(app.HOMELAB_MCP_OPENBAO_JWT_PATH, "/var/run/secrets/homelab-mcp/openbao/token");
    assert.equal(app.HOMELAB_MCP_OPENBAO_REQUEST_TIMEOUT_MS, "5000");
    assert.deepEqual(JSON.parse(app.HOMELAB_MCP_KUBERNETES_CLUSTERS as string), [
      {
        name: "pantheon",
        kubeconfig: "/var/run/secrets/homelab-mcp/kubernetes/kubeconfig",
        context: "pantheon",
        cache_dir: "/tmp/kubectl/pantheon",
      },
      {
        name: "romulus",
        kubeconfig: "/var/run/secrets/homelab-mcp/kubernetes/kubeconfig",
        context: "romulus",
        cache_dir: "/tmp/kubectl/romulus",
      },
    ]);
    assert.deepEqual(JSON.parse(app.HOMELAB_MCP_CEPH_CLUSTERS as string), [
      {
        name: "romulus",
        origin: "https://ceph.romulus.example.test",
        expected_major_release: 19,
        username_env: "HOMELAB_MCP_CEPH_ROMULUS_USERNAME",
        password_env: "HOMELAB_MCP_CEPH_ROMULUS_PASSWORD",
      },
      {
        name: "pantheon",
        origin: "https://ceph.pantheon.example.test",
        expected_major_release: 19,
        username_env: "HOMELAB_MCP_CEPH_PANTHEON_USERNAME",
        password_env: "HOMELAB_MCP_CEPH_PANTHEON_PASSWORD",
      },
    ]);
    assert.equal(app.HOMELAB_MCP_CEPH_ROMULUS_USERNAME, cephUsernameFixture);
    assert.equal(app.HOMELAB_MCP_CEPH_ROMULUS_PASSWORD, cephPasswordFixture);
    assert.equal(app.HOMELAB_MCP_CEPH_PANTHEON_USERNAME, cephUsernameFixture);
    assert.equal(app.HOMELAB_MCP_CEPH_PANTHEON_PASSWORD, cephPasswordFixture);
    assert.equal(app.HOMELAB_MCP_OAUTH_ALLOW_DCR, "true");
    assert.equal(app.HOMELAB_MCP_OAUTH_ALLOW_CIMD, "true");
    assert.equal(
      app.HOMELAB_MCP_OAUTH_CIMD_TRUSTED_PRIVATE_ORIGINS,
      "https://kuri.internal.example",
    );
    assert.equal(app.HOMELAB_MCP_OAUTH_ALLOW_LOOPBACK_REDIRECTS, "true");
    assert.equal(
      app.HOMELAB_MCP_OAUTH_WRAPPING_KEYS_FILE,
      "/var/run/secrets/homelab-mcp/oauth/keyring.json",
    );
    assert.equal(app.HOMELAB_MCP_GRAFANA_URL, "https://grafana.example.test");
    assert.equal(app.HOMELAB_MCP_GRAFANA_TOKEN, "test-grafana-token");
    assert.equal(
      app.HOMELAB_MCP_FORGEJO_ORIGIN,
      "https://git.holdenitdown.net",
    );
    assert.equal(app.HOMELAB_MCP_FORGEJO_TOKEN, forgejoTokenFixture);
    assert.equal(app.HOMELAB_MCP_TEKTON_NAMESPACE, "pipelines-as-code");
    assert.equal(
      app.HOMELAB_MCP_PAC_URL,
      "http://pipelines-as-code-controller.pipelines-as-code.svc.cluster.local:8080",
    );
    assert.equal(
      app.HOMELAB_MCP_PAC_INCOMING_SECRET,
      pacIncomingSecretFixture,
    );
    assert.equal(app.HOMELAB_MCP_DEPLOYMENT_ENVIRONMENT, "test");
    assert.equal(app.HOMELAB_MCP_SERVICE_NAMESPACE, "homelab");
    assert.equal(
      app.HOMELAB_MCP_PYROSCOPE_URL,
      "https://telemetry.holdenitdown.net:4040",
    );
    assert.equal(
      app.OTEL_EXPORTER_OTLP_ENDPOINT,
      "https://telemetry.holdenitdown.net:4318",
    );
    assert.equal(app.OTEL_EXPORTER_OTLP_PROTOCOL, "http/protobuf");
    assert.equal(app.OTEL_SERVICE_NAME, "homelab-mcp");
    assert.equal(
      app.OTEL_RESOURCE_ATTRIBUTES,
      "service.namespace=homelab,deployment.environment.name=test",
    );

    for (const candidate of resources.filter(
      (entry) =>
        entry.type !== "kubernetes:core/v1:Secret" &&
        entry.type !== "pulumi:index:Stash",
    )) {
      assert.doesNotMatch(
        JSON.stringify(candidate.inputs),
        /HOMELAB_MCP_(?:DATABASE_URL|OIDC_CLIENT_SECRET|GRAFANA_TOKEN|FORGEJO_TOKEN|PAC_INCOMING_SECRET|CEPH_[A-Z_]+_(?:USERNAME|PASSWORD))/,
      );
      assert.doesNotMatch(JSON.stringify(candidate.inputs), /test-grafana-token/);
      assert.doesNotMatch(JSON.stringify(candidate.inputs), /test-forgejo-token/);
      assert.doesNotMatch(
        JSON.stringify(candidate.inputs),
        /test-pac-incoming-secret/,
      );
      assert.doesNotMatch(JSON.stringify(candidate.inputs), /test-ceph-(?:user|password)/);
    }
  });

  test("stashes secret seed inputs and projects only their outputs", () => {
    const forgejo = resource("pulumi:index:Stash", "homelab-mcp-forgejo-token");
    const pac = resource(
      "pulumi:index:Stash",
      "homelab-mcp-pac-incoming-secret",
    );
    assert.ok(isSecret(forgejo.inputs.input));
    assert.ok(isSecret(pac.inputs.input));
    assert.equal(unwrapSecrets(forgejo.inputs.input), forgejoTokenFixture);
    assert.equal(unwrapSecrets(pac.inputs.input), pacIncomingSecretFixture);

    for (const cluster of ["romulus", "pantheon"]) {
      const username = resource(
        "pulumi:index:Stash",
        `homelab-mcp-ceph-${cluster}-username`,
      );
      const password = resource(
        "pulumi:index:Stash",
        `homelab-mcp-ceph-${cluster}-password`,
      );
      assert.ok(isSecret(username.inputs.input));
      assert.ok(isSecret(password.inputs.input));
      assert.equal(unwrapSecrets(username.inputs.input), cephUsernameFixture);
      assert.equal(unwrapSecrets(password.inputs.input), cephPasswordFixture);
    }

    const appSecret = resource(
      "kubernetes:core/v1:Secret",
      "homelab-mcp-app",
    );
    assert.ok(isSecret(appSecret.inputs.stringData));
    const app = unwrapSecrets(appSecret.inputs.stringData) as Record<
      string,
      unknown
    >;
    assert.equal(app.HOMELAB_MCP_FORGEJO_TOKEN, forgejoTokenFixture);
    assert.equal(
      app.HOMELAB_MCP_PAC_INCOMING_SECRET,
      pacIncomingSecretFixture,
    );
    assert.equal(app.HOMELAB_MCP_CEPH_ROMULUS_USERNAME, cephUsernameFixture);
    assert.equal(app.HOMELAB_MCP_CEPH_ROMULUS_PASSWORD, cephPasswordFixture);
    assert.equal(app.HOMELAB_MCP_CEPH_PANTHEON_USERNAME, cephUsernameFixture);
    assert.equal(app.HOMELAB_MCP_CEPH_PANTHEON_PASSWORD, cephPasswordFixture);
  });

  test("uses an explicit bounded service-account credential projection", () => {
    const account = resource(
      "kubernetes:core/v1:ServiceAccount",
      "homelab-mcp",
    );
    assert.equal((account.inputs.metadata as any).name, "homelab-mcp");
    assert.equal(
      (account.inputs.metadata as any).namespace,
      "homelab-mcp-test",
    );
    assert.equal(account.inputs.automountServiceAccountToken, false);

    const deployment = resource(
      "kubernetes:apps/v1:Deployment",
      "homelab-mcp",
    );
    const pod = (unwrapSecrets(deployment.inputs.spec) as any).template.spec;
    assert.equal(pod.automountServiceAccountToken, false);
    assert.equal(pod.serviceAccountName, "homelab-mcp");
    assert.deepEqual(
      pod.containers[0].volumeMounts.find(
        (mount: any) => mount.name === "kube-api-access",
      ),
      {
        name: "kube-api-access",
        mountPath: "/var/run/secrets/kubernetes.io/serviceaccount",
        readOnly: true,
      },
    );
    assert.deepEqual(
      pod.volumes.find((volume: any) => volume.name === "kube-api-access"),
      {
        name: "kube-api-access",
        projected: {
          defaultMode: 0o444,
          sources: [
            {
              serviceAccountToken: {
                expirationSeconds: 3600,
                path: "token",
              },
            },
            {
              configMap: {
                name: "kube-root-ca.crt",
                items: [{ key: "ca.crt", path: "ca.crt" }],
              },
            },
            {
              downwardAPI: {
                items: [
                  {
                    path: "namespace",
                    fieldRef: {
                      apiVersion: "v1",
                      fieldPath: "metadata.namespace",
                    },
                  },
                ],
              },
            },
          ],
        },
      },
    );
  });

  test("creates least-privilege OpenBao preview resources", () => {
    const provider = resource("pulumi:providers:vault", "homelab-mcp-openbao");
    assert.equal(provider.inputs.address, "https://openbao.example.test");
    assert.equal(provider.inputs.authLoginTokenFile, undefined);
    assert.equal(provider.inputs.authLogin, undefined);
    assert.equal(provider.inputs.skipChildToken, "true");
    assert.equal(provider.inputs.token, undefined);
    assert.equal(resources.some((entry) => entry.type === "vault:index/authBackend:AuthBackend"), false);
    const mount = resource("vault:index/mount:Mount", "homelab-mcp-openbao-ssh");
    assert.deepEqual({ type: mount.inputs.type, path: mount.inputs.path, defaultLeaseTtlSeconds: mount.inputs.defaultLeaseTtlSeconds, maxLeaseTtlSeconds: mount.inputs.maxLeaseTtlSeconds }, { type: "ssh", path: "homelab-ssh-client", defaultLeaseTtlSeconds: 900, maxLeaseTtlSeconds: 900 });
    const ca = resource("vault:ssh/secretBackendCa:SecretBackendCa", "homelab-mcp-openbao-ssh-ca");
    assert.equal(ca.inputs.generateSigningKey, true);
    assert.equal(ca.inputs.privateKey, undefined);
    const sshRole = resource("vault:ssh/secretBackendRole:SecretBackendRole", "homelab-mcp-openbao-ssh-role");
    assert.deepEqual({ keyType: sshRole.inputs.keyType, allowUserCertificates: sshRole.inputs.allowUserCertificates, allowHostCertificates: sshRole.inputs.allowHostCertificates, allowedUsers: sshRole.inputs.allowedUsers, defaultUser: sshRole.inputs.defaultUser, ttl: sshRole.inputs.ttl, maxTtl: sshRole.inputs.maxTtl, notBeforeDuration: sshRole.inputs.notBeforeDuration }, { keyType: "ca", allowUserCertificates: true, allowHostCertificates: false, allowedUsers: "homelab", defaultUser: "homelab", ttl: "15m", maxTtl: "15m", notBeforeDuration: "30s" });
    const policy = resource("vault:index/policy:Policy", "homelab-mcp-openbao");
    assert.equal(policy.inputs.policy, 'path "homelab-ssh-client/sign/homelab" {\n  capabilities = ["update"]\n}\n');
    const role = resource("vault:kubernetes/authBackendRole:AuthBackendRole", "homelab-mcp-openbao-kubernetes-role");
    assert.deepEqual({ audience: role.inputs.audience, boundServiceAccountNames: role.inputs.boundServiceAccountNames, boundServiceAccountNamespaces: role.inputs.boundServiceAccountNamespaces, tokenPolicies: role.inputs.tokenPolicies, tokenNoDefaultPolicy: role.inputs.tokenNoDefaultPolicy, tokenTtl: role.inputs.tokenTtl, tokenMaxTtl: role.inputs.tokenMaxTtl }, { audience: "https://openbao.example.test", boundServiceAccountNames: ["homelab-mcp"], boundServiceAccountNamespaces: ["homelab-mcp-test"], tokenPolicies: ["homelab-mcp-test"], tokenNoDefaultPolicy: true, tokenTtl: 900, tokenMaxTtl: 900 });
  });

  test("projects an audience-bound OpenBao JWT and memory-only credential directory", () => {
    const deployment = resource("kubernetes:apps/v1:Deployment", "homelab-mcp");
    const pod = (unwrapSecrets(deployment.inputs.spec) as any).template.spec;
    assert.deepEqual(pod.containers[0].volumeMounts.find((mount: any) => mount.name === "openbao-jwt"), { name: "openbao-jwt", mountPath: "/var/run/secrets/homelab-mcp/openbao", readOnly: true });
    assert.deepEqual(pod.volumes.find((volume: any) => volume.name === "openbao-jwt"), { name: "openbao-jwt", projected: { defaultMode: 0o440, sources: [{ serviceAccountToken: { audience: "https://openbao.example.test", expirationSeconds: 600, path: "token" } }] } });
    assert.deepEqual(pod.containers[0].volumeMounts.find((mount: any) => mount.name === "deploy-credentials"), { name: "deploy-credentials", mountPath: "/var/run/homelab-mcp/deploy" });
    assert.deepEqual(pod.volumes.find((volume: any) => volume.name === "deploy-credentials"), { name: "deploy-credentials", emptyDir: { medium: "Memory", sizeLimit: "16Mi" } });
    assert.equal(pod.securityContext.runAsUser, 65532);
    const allRules = resources.filter((entry) => entry.type.includes("Role")).flatMap((entry) => (entry.inputs.rules as any[]) ?? []);
    assert.equal(allRules.some((rule) => rule.resources?.includes("secrets")), false);
  });

  test("grants exact namespace-scoped Tekton and PAC permissions", () => {
    const role = resource(
      "kubernetes:rbac.authorization.k8s.io/v1:Role",
      "homelab-mcp-tekton",
    );
    assert.equal((role.inputs.metadata as any).namespace, "pipelines-as-code");
    assert.deepEqual(role.inputs.rules, [
      {
        apiGroups: ["tekton.dev"],
        resources: ["pipelineruns"],
        verbs: ["get", "list", "patch"],
      },
      {
        apiGroups: ["tekton.dev"],
        resources: ["taskruns"],
        verbs: ["get", "list"],
      },
      {
        apiGroups: ["pipelinesascode.tekton.dev"],
        resources: ["repositories"],
        verbs: ["get", "list"],
      },
      { apiGroups: [""], resources: ["pods"], verbs: ["get"] },
      { apiGroups: [""], resources: ["pods/log"], verbs: ["get"] },
    ]);

    const binding = resource(
      "kubernetes:rbac.authorization.k8s.io/v1:RoleBinding",
      "homelab-mcp-tekton",
    );
    assert.equal((binding.inputs.metadata as any).namespace, "pipelines-as-code");
    assert.deepEqual((binding.inputs.metadata as any).annotations, {
      "pulumi.com/skipAwait": "true",
    });
    assert.deepEqual(binding.inputs.roleRef, {
      apiGroup: "rbac.authorization.k8s.io",
      kind: "Role",
      name: "homelab-mcp",
    });
    assert.deepEqual(binding.inputs.subjects, [
      {
        kind: "ServiceAccount",
        name: "homelab-mcp",
        namespace: "homelab-mcp-test",
      },
    ]);

    const rules = role.inputs.rules as Array<{ resources: string[]; verbs: string[] }>;
    assert.equal(rules.some((rule) => rule.resources.includes("secrets")), false);
    assert.equal(
      rules.some((rule) =>
        rule.verbs.some((verb) => ["create", "delete", "update"].includes(verb)),
      ),
      false,
    );
  });

  test("creates provider-bound runtime identities and exact Kubernetes RBAC per cluster", () => {
    const providers = resources.filter(
      (candidate) => candidate.type === "pulumi:providers:kubernetes",
    );
    assert.deepEqual(
      providers.map((provider) => provider.inputs.context).sort(),
      ["pantheon", "romulus"],
    );
    for (const candidate of resources.filter((entry) =>
      entry.type.startsWith("kubernetes:"),
    )) {
      assert.match(
        candidate.provider ?? "",
        /homelab-mcp-(?:pantheon|romulus)/,
      );
    }

    for (const cluster of ["pantheon", "romulus"]) {
      const account = resource(
        "kubernetes:core/v1:ServiceAccount",
        `homelab-mcp-kubernetes-runtime-${cluster}`,
      );
      assert.equal(account.inputs.automountServiceAccountToken, false);
      assert.equal(
        (account.inputs.metadata as any).name,
        "homelab-mcp-test-kubernetes-runtime",
      );
      const binding = resource(
        "kubernetes:rbac.authorization.k8s.io/v1:ClusterRoleBinding",
        `homelab-mcp-kubernetes-runtime-${cluster}`,
      );
      assert.equal(binding.inputs.roleRef && (binding.inputs.roleRef as any).name,
        "homelab-mcp-test-kubernetes-runtime");
      assert.equal((binding.inputs.subjects as any[])[0].name,
        "homelab-mcp-test-kubernetes-runtime");
      const token = resource(
        "kubernetes:core/v1:Secret",
        `homelab-mcp-kubernetes-runtime-token-${cluster}`,
      );
      assert.equal(token.inputs.type, "kubernetes.io/service-account-token");
      assert.deepEqual((token.inputs.metadata as any).annotations, {
        "kubernetes.io/service-account.name":
          "homelab-mcp-test-kubernetes-runtime",
        "pulumi.com/waitFor": "jsonpath={.data.token}",
      });
    }

    const expectedRules = [
      { nonResourceURLs: ["/api", "/apis", "/version", "/api/v1", "/apis/apps/v1", "/apis/autoscaling/v2", "/apis/batch/v1", "/apis/ceph.rook.io/v1", "/apis/cert-manager.io/v1", "/apis/discovery.k8s.io/v1", "/apis/events.k8s.io/v1", "/apis/gateway.networking.k8s.io/v1", "/apis/kafka.strimzi.io/v1beta2", "/apis/metrics.k8s.io/v1beta1", "/apis/networking.k8s.io/v1", "/apis/policy/v1", "/apis/postgresql.cnpg.io/v1", "/apis/storage.k8s.io/v1", "/apis/velero.io/v1"], verbs: ["get"] },
      { apiGroups: [""], resources: ["namespaces", "nodes", "pods", "services", "persistentvolumeclaims"], verbs: ["get", "list"] },
      { apiGroups: ["events.k8s.io"], resources: ["events"], verbs: ["get", "list"] },
      { apiGroups: ["apps"], resources: ["deployments", "statefulsets", "daemonsets", "replicasets"], verbs: ["get", "list"] },
      { apiGroups: ["batch"], resources: ["jobs", "cronjobs"], verbs: ["get", "list"] },
      { apiGroups: ["metrics.k8s.io"], resources: ["pods", "nodes"], verbs: ["get", "list"] },
      { apiGroups: ["discovery.k8s.io"], resources: ["endpointslices"], verbs: ["get", "list"] },
      { apiGroups: ["networking.k8s.io"], resources: ["ingresses", "networkpolicies"], verbs: ["get", "list"] },
      { apiGroups: ["gateway.networking.k8s.io"], resources: ["gatewayclasses", "gateways", "httproutes"], verbs: ["get", "list"] },
      { apiGroups: ["autoscaling"], resources: ["horizontalpodautoscalers"], verbs: ["get", "list"] },
      { apiGroups: ["policy"], resources: ["poddisruptionbudgets"], verbs: ["get", "list"] },
      { apiGroups: ["storage.k8s.io"], resources: ["storageclasses"], verbs: ["get", "list"] },
      { apiGroups: ["cert-manager.io"], resources: ["certificates", "clusterissuers"], verbs: ["get", "list"] },
      { apiGroups: ["postgresql.cnpg.io"], resources: ["clusters"], verbs: ["get", "list"] },
      { apiGroups: ["kafka.strimzi.io"], resources: ["kafkas", "kafkanodepools", "kafkatopics"], verbs: ["get", "list"] },
      { apiGroups: ["ceph.rook.io"], resources: ["cephclusters", "cephfilesystems", "cephblockpools", "cephobjectstores"], verbs: ["get", "list"] },
      { apiGroups: ["velero.io"], resources: ["backups", "schedules", "backupstoragelocations"], verbs: ["get", "list"] },
      { apiGroups: ["apps"], resources: ["deployments", "statefulsets", "daemonsets"], verbs: ["patch"] },
      { apiGroups: ["apps"], resources: ["deployments/scale", "statefulsets/scale"], verbs: ["get", "patch"] },
      { apiGroups: ["batch"], resources: ["cronjobs"], verbs: ["patch"] },
      { apiGroups: ["batch"], resources: ["jobs"], verbs: ["create"] },
      { apiGroups: [""], resources: ["pods"], verbs: ["delete"] },
    ];
    for (const cluster of ["pantheon", "romulus"]) {
      const role = resource(
        "kubernetes:rbac.authorization.k8s.io/v1:ClusterRole",
        `homelab-mcp-kubernetes-runtime-${cluster}`,
      );
      assert.deepEqual(role.inputs.rules, expectedRules);
      const serialized = JSON.stringify(role.inputs.rules);
      assert.doesNotMatch(serialized, /(?:"\*"|secrets|configmaps|pods\/log|exec|attach)/);
      assert.doesNotMatch(serialized, /token/);
    }
  });

  test("assembles a secret-tainted reduced runtime kubeconfig and mount", () => {
    const kubeconfigSecret = resource(
      "kubernetes:core/v1:Secret",
      "homelab-mcp-kubernetes-runtime-kubeconfig",
    );
    assert.ok(isSecret(kubeconfigSecret.inputs.stringData));
    const stringData = unwrapSecrets(kubeconfigSecret.inputs.stringData) as Record<string, string>;
    const kubeconfig = JSON.parse(stringData.kubeconfig);
    assert.equal(kubeconfig.clusters.length, 2);
    assert.equal(kubeconfig.users.length, 2);
    assert.deepEqual(
      kubeconfig.clusters.map((entry: any) => [entry.name, entry.cluster.server]),
      [
        ["pantheon", "https://pantheon.example.test:6443"],
        ["romulus", "https://romulus.example.test"],
      ],
    );
    assert.equal(kubeconfig["current-context"], "pantheon");
    assert.equal(kubeconfig.clusters[0].cluster["certificate-authority"], undefined);
    assert.equal(kubeconfig.users[0].user["client-certificate-data"], undefined);

    const deployment = resource(
      "kubernetes:apps/v1:Deployment",
      "homelab-mcp",
    );
    const pod = (unwrapSecrets(deployment.inputs.spec) as any).template.spec;
    assert.deepEqual(
      pod.containers[0].volumeMounts.find(
        (mount: any) => mount.name === "kubernetes-runtime-kubeconfig",
      ),
      {
        name: "kubernetes-runtime-kubeconfig",
        mountPath: "/var/run/secrets/homelab-mcp/kubernetes",
        readOnly: true,
      },
    );
    assert.deepEqual(
      pod.volumes.find(
        (volume: any) => volume.name === "kubernetes-runtime-kubeconfig",
      ).secret.items,
      [{ key: "kubeconfig", path: "kubeconfig", mode: 0o440 }],
    );
    assert.equal(pod.securityContext.fsGroup, 65532);
    assert.ok(pod.volumes.some((volume: any) => volume.name === "tmp"));
  });

  test("creates a versioned 32-byte keyring with checksum rollout", () => {
    const randomBytes = resource(
      "random:index/randomBytes:RandomBytes",
      "homelab-mcp-oauth-wrapping-key-v1",
    );
    assert.equal(randomBytes.inputs.length, 32);
    const keyringSecret = resource(
      "kubernetes:core/v1:Secret",
      "homelab-mcp-oauth-wrapping-keys",
    );
    assert.ok(isSecret(keyringSecret.inputs.stringData));
    const data = unwrapSecrets(keyringSecret.inputs.stringData) as Record<
      string,
      string
    >;
    const keyring = JSON.parse(data["keyring.json"]);
    assert.equal(keyring.schema_version, 1);
    assert.equal(keyring.active, "v1");
    assert.deepEqual(keyring.keys.map((key: any) => key.id), ["v1"]);

    const deployment = resource(
      "kubernetes:apps/v1:Deployment",
      "homelab-mcp",
    );
    const podTemplate = (unwrapSecrets(deployment.inputs.spec) as any).template;
    assert.notEqual(
      podTemplate.metadata.annotations[
        "homelab-mcp.holdenitdown.net/wrapping-key-checksum"
      ],
      undefined,
    );
    assert.deepEqual(
      podTemplate.spec.containers[0].volumeMounts.find(
        (mount: any) => mount.name === "oauth-wrapping-keys",
      ),
      {
        name: "oauth-wrapping-keys",
        mountPath: "/var/run/secrets/homelab-mcp/oauth",
        readOnly: true,
      },
    );
  });

  test("hardens the one-replica Recreate workload and configures probes", () => {
    const deployment = resource(
      "kubernetes:apps/v1:Deployment",
      "homelab-mcp",
    );
    const spec = unwrapSecrets(deployment.inputs.spec) as any;
    assert.equal(spec.replicas, 1);
    assert.equal(spec.strategy.type, "Recreate");
    assert.equal(
      deployment.inputs.metadata &&
        (deployment.inputs.metadata as any).annotations[
          "secret.reloader.stakater.com/reload"
        ],
      "homelab-mcp-app,homelab-mcp-oauth-wrapping-keys,homelab-mcp-kubernetes-runtime-kubeconfig",
    );
    const pod = spec.template.spec;
    assert.deepEqual(spec.template.metadata.annotations, {
      "homelab-mcp.holdenitdown.net/wrapping-key-checksum":
        spec.template.metadata.annotations[
          "homelab-mcp.holdenitdown.net/wrapping-key-checksum"
        ],
      "resource.opentelemetry.io/service.name": "homelab-mcp",
      "resource.opentelemetry.io/service.namespace": "homelab",
      "resource.opentelemetry.io/deployment.environment.name": "test",
    });
    assert.equal(pod.automountServiceAccountToken, false);
    assert.equal(pod.securityContext.runAsUser, 65532);
    assert.equal(pod.securityContext.runAsGroup, 65532);
    assert.equal(pod.securityContext.seccompProfile.type, "RuntimeDefault");
    const container = pod.containers[0];
    assert.match(container.image, /@sha256:[a-f0-9]{64}$/);
    assert.equal(container.ports[0].containerPort, 14333);
    assert.deepEqual(container.envFrom, [
      { secretRef: { name: "homelab-mcp-app" } },
    ]);
    assert.deepEqual(container.env, [
      {
        name: "HOMELAB_MCP_K8S_NAMESPACE",
        valueFrom: { fieldRef: { fieldPath: "metadata.namespace" } },
      },
      {
        name: "HOMELAB_MCP_K8S_POD_NAME",
        valueFrom: { fieldRef: { fieldPath: "metadata.name" } },
      },
      {
        name: "HOMELAB_MCP_K8S_POD_UID",
        valueFrom: { fieldRef: { fieldPath: "metadata.uid" } },
      },
    ]);
    assert.equal(container.securityContext.allowPrivilegeEscalation, false);
    assert.equal(container.securityContext.readOnlyRootFilesystem, true);
    assert.deepEqual(container.securityContext.capabilities.drop, ["ALL"]);
    assert.equal(container.startupProbe.httpGet.path, "/health");
    assert.equal(container.livenessProbe.httpGet.path, "/health");
    assert.equal(container.readinessProbe.httpGet.path, "/ready");
    assert.deepEqual(container.resources, {
      requests: { cpu: "50m", memory: "64Mi" },
      limits: { cpu: "500m", memory: "256Mi" },
    });
  });

  test("allows patterned DNS, HTTPS, telemetry, Traefik, and PostgreSQL egress", () => {
    const policy = resource(
      "kubernetes:networking.k8s.io/v1:NetworkPolicy",
      "homelab-mcp-egress",
    );
    const spec = policy.inputs.spec as any;
    assert.deepEqual(spec.policyTypes, ["Egress"]);
    assert.deepEqual(spec.egress, [
      {
        to: [
          {
            namespaceSelector: {
              matchLabels: { "kubernetes.io/metadata.name": "kube-system" },
            },
          },
        ],
        ports: [
          { port: 53, protocol: "UDP" },
          { port: 53, protocol: "TCP" },
        ],
      },
      {
        to: [{
          ipBlock: {
            cidr: "0.0.0.0/0",
            except: ["172.16.5.10/32"],
          },
        }],
        ports: [{ port: 443, protocol: "TCP" }],
      },
      {
        to: [{ ipBlock: { cidr: "172.16.5.10/32" } }],
        ports: [{ port: 8200, protocol: "TCP" }],
      },
      {
        to: [{ ipBlock: { cidr: "172.16.0.0/16" } }],
        ports: [{ port: 22, protocol: "TCP" }],
      },
      {
        ports: [
          { port: 4040, protocol: "TCP" },
          { port: 4318, protocol: "TCP" },
        ],
      },
      {
        to: [{ ipBlock: { cidr: "172.16.3.0/24" } }],
        ports: [{ port: 6443, protocol: "TCP" }],
      },
      {
        to: [{ ipBlock: { cidr: "172.16.4.0/24" } }],
        ports: [{ port: 443, protocol: "TCP" }],
      },
      {
        to: [
          {
            namespaceSelector: {
              matchLabels: { "kubernetes.io/metadata.name": "ingress" },
            },
            podSelector: {
              matchLabels: {
                "app.kubernetes.io/name": "traefik",
                "app.kubernetes.io/instance":
                  "cluster-ingress-ingress-chart-ingress",
              },
            },
          },
        ],
        ports: [{ port: 8443, protocol: "TCP" }],
      },
      {
        to: [
          {
            podSelector: {
              matchLabels: { "cnpg.io/cluster": "homelab-mcp-postgres" },
            },
          },
        ],
        ports: [{ port: 5432, protocol: "TCP" }],
      },
      {
        to: [
          {
            namespaceSelector: {
              matchLabels: {
                "kubernetes.io/metadata.name": "pipelines-as-code",
              },
            },
          },
        ],
        ports: [{ port: 8082, protocol: "TCP" }],
      },
    ]);
    const generalHttps = spec.egress.find((rule: any) =>
      rule.ports?.some((port: any) => port.port === 443) &&
      rule.to?.[0]?.ipBlock?.cidr === "0.0.0.0/0",
    );
    const openbao = spec.egress.find((rule: any) =>
      rule.ports?.some((port: any) => port.port === 8200) &&
      rule.to?.[0]?.ipBlock?.cidr === "172.16.5.10/32",
    );
    assert.deepEqual(generalHttps.to[0].ipBlock.except, ["172.16.5.10/32"]);
    assert.ok(openbao);
    const machineSsh = spec.egress.filter((rule: any) =>
      rule.ports?.some((port: any) => port.port === 22),
    );
    assert.deepEqual(machineSsh, [
      {
        to: [{ ipBlock: { cidr: "172.16.0.0/16" } }],
        ports: [{ port: 22, protocol: "TCP" }],
      },
    ]);
    assert.equal(
      spec.egress.some((rule: any) =>
        rule.ports?.some(
          (port: any) => port.port === 22 && port.endPort !== undefined,
        ) ||
        (rule.ports?.some((port: any) => port.port === 22) && !rule.to),
      ),
      false,
    );
    assert.equal(
      spec.egress.some((rule: any) =>
        rule.ports?.some((port: any) => port.port === 443) && !rule.to,
      ),
      false,
    );
  });

  test("creates the ClusterIP service and streaming Gateway API route", () => {
    const service = resource("kubernetes:core/v1:Service", "homelab-mcp");
    assert.equal((service.inputs.spec as any).type, "ClusterIP");
    assert.equal((service.inputs.spec as any).ports[0].port, 14333);
    assert.equal((service.inputs.spec as any).ports[0].appProtocol, undefined);
    const route = resourceByName("homelab-mcp-route").inputs;
    assert.equal(route.kind, "HTTPRoute");
    assert.deepEqual((route.spec as any).parentRefs[0], {
      group: "gateway.networking.k8s.io",
      kind: "Gateway",
      name: "default-gateway",
      namespace: "ingress",
    });
    assert.deepEqual((route.spec as any).hostnames, [
      "homelab-mcp.example.test",
    ]);
    assert.equal((route.spec as any).rules[0].timeouts.request, "0s");
  });

  test("exports only secret-safe discovery and location values", () => {
    assert.deepEqual(Object.keys(program).sort(), [
      "mcpOAuthIssuer",
      "mcpOAuthJwksUrl",
      "mcpOAuthMetadataUrl",
      "mcpOAuthResource",
      "namespaceNameOutput",
      "publicUrlOutput",
    ]);
    const source = readFileSync(join(__dirname, "index.ts"), "utf8");
    assert.doesNotMatch(source, /export const .*?(?:token|password|secret|key)/i);
  });

  test("pipeline applies the separately previewed stack without an inline preview", () => {
    const pipeline = readFileSync(
      join(__dirname, "..", "..", ".tekton", "homelab-mcp-preview.yaml"),
      "utf8",
    );
    assert.doesNotMatch(pipeline, /pulumi preview --stack preview/);
    const apply = pipeline
      .split("\n")
      .find((line) => line.includes("pulumi up --stack preview"));
    assert.ok(apply);
    assert.match(apply, /--yes --skip-preview/);
    assert.match(
      pipeline,
      /\/usr\/local\/bin\/kubectl version --client --output=json/,
    );
    assert.match(pipeline, /"gitVersion":"v1\.33\.5"/);
  });

  test("pins and verifies the multi-architecture kubectl image input", () => {
    const dockerfile = readFileSync(
      join(__dirname, "..", "..", "Dockerfile"),
      "utf8",
    );
    assert.match(dockerfile, /ARG KUBECTL_VERSION=v1\.33\.5/);
    assert.match(dockerfile, /COPY migrations migrations\n[\s\S]*cargo build --locked --release/);
    assert.match(
      dockerfile,
      /6a12d6c39e4a611a3687ee24d8c733961bb4bae1ae975f5204400c0a6930c6fc/,
    );
    assert.match(
      dockerfile,
      /6db7c5d846c3b3ddfd39f3137a93fe96af3938860eefdbf2429805ee1656e381/,
    );
    assert.match(
      dockerfile,
      /https:\/\/dl\.k8s\.io\/release\/\$\{KUBECTL_VERSION\}\/bin\/linux\/\$\{TARGETARCH\}\/kubectl/,
    );
    assert.match(dockerfile, /sha256sum --check --strict/);
    assert.match(
      dockerfile,
      /COPY --from=kubectl \/usr\/local\/bin\/kubectl \/usr\/local\/bin\/kubectl/,
    );
    assert.doesNotMatch(dockerfile, /(?:stable\.txt|latest|apt-get install[^\n]*kubectl)/);
  });

  test("packages a locked offline deploy runtime and isolates the CI OpenBao identity", () => {
    const dockerfile = readFileSync(join(__dirname, "..", "..", "Dockerfile"), "utf8");
    assert.match(dockerfile, /ARG UV_VERSION=0\.11\.15/);
    assert.match(dockerfile, /uv-\$\{UV_VERSION\}\.data\/scripts\/uv \/usr\/local\/bin\/uv/);
    assert.match(dockerfile, /uv sync --locked --no-dev --no-install-project/);
    assert.match(dockerfile, /UV_OFFLINE=1/);
    assert.match(dockerfile, /UV_NO_SYNC=1/);
    assert.match(dockerfile, /COPY deploys deploys/);
    assert.match(dockerfile, /openssh-client/);
    const pipeline = readFileSync(join(__dirname, "..", "..", ".tekton", "homelab-mcp-preview.yaml"), "utf8");
    const taskRunSpecs = pipeline.slice(
      pipeline.indexOf("  taskRunSpecs:"),
      pipeline.indexOf("  timeouts:"),
    );
    assert.match(taskRunSpecs, /pipelineTaskName: deploy-preview\n\s+serviceAccountName: openbao-pulumi-admin-v1/);
    assert.equal((pipeline.match(/serviceAccountName: openbao-pulumi-admin-v1/g) ?? []).length, 1);
    assert.match(taskRunSpecs, /serviceAccountName: openbao-pulumi-admin-v1\n\s+timeout: 25m0s/);
    assert.equal((pipeline.match(/timeout: 25m0s/g) ?? []).length, 1);
    assert.match(taskRunSpecs, /serviceAccountName: openbao-pulumi-admin-v1[\s\S]*kubernetes\.io\/arch: amd64/);

    const deployStart = pipeline.indexOf("      - name: deploy-preview");
    const deployEnd = pipeline.indexOf("\n        params:", deployStart);
    assert.ok(deployStart >= 0 && deployEnd > deployStart);
    const deployTask = pipeline.slice(deployStart, deployEnd);
    const outsideDeployTask = pipeline.slice(0, deployStart) + pipeline.slice(deployEnd);
    assert.match(deployTask, /name: openbao-ci-jwt[\s\S]*defaultMode: 256[\s\S]*serviceAccountToken:[\s\S]*path: token[\s\S]*audience: openbao-pulumi-admin-v1[\s\S]*expirationSeconds: 600/);
    assert.match(deployTask, /name: openbao-ci-session\n\s+emptyDir:\n\s+medium: Memory\n\s+sizeLimit: 64Ki/);
    assert.match(deployTask, /name: openbao-login\n\s+ref:\n\s+name: openbao-kubernetes-login/);
    assert.match(deployTask, /name: role\n\s+value: openbao-pulumi-admin-v1\n\s+- name: expected-policy\n\s+value: openbao-pulumi-admin\n\s+- name: max-lease-seconds\n\s+value: "1800"/);
    assert.doesNotMatch(deployTask, /name: (?:jwt-volume-name|session-volume-name)/);
    assert.match(deployTask, /secretName: tekton-cluster-kubeconfig/);
    assert.match(deployTask, /name: KUBECONFIG\n\s+value: \/etc\/kubeconfig\/kubeconfig/);
    assert.match(deployTask, /name: pulumi-credentials/);
    assert.match(deployTask, /name: authentik-credentials/);
    assert.match(deployTask, /name: grafana-credentials/);
    const applyStart = deployTask.indexOf("            - name: apply-preview");
    assert.ok(applyStart >= 0);
    const applyStep = deployTask.slice(applyStart);
    assert.match(applyStep, /name: openbao-ci-session\n\s+mountPath: \/var\/run\/secrets\/openbao-ci\/session/);
    assert.doesNotMatch(applyStep, /(?:openbao-ci-jwt|\/var\/run\/secrets\/openbao-ci\/jwt)/);
    assert.match(applyStep, /\$\{VAULT_TOKEN\+x\}/);
    assert.match(applyStep, /\$\{BAO_TOKEN\+x\}/);
    assert.match(applyStep, /token_file=\/var\/run\/secrets\/openbao-ci\/session\/token/);
    assert.match(applyStep, /test -f "\$token_file"[\s\S]*test -s "\$token_file"/);
    assert.match(applyStep, /stat -c '%a' "\$token_file"`" = 400/);
    assert.match(applyStep, /VAULT_TOKEN="`cat "\$token_file"`"/);
    assert.match(applyStep, /test -n "\$VAULT_TOKEN"[\s\S]*test "\$\{#VAULT_TOKEN\}" -le 4096/);
    assert.match(applyStep, /export VAULT_TOKEN/);
    assert.match(applyStep, /unset VAULT_TOKEN/);
    assert.match(applyStep, /rm -f "\$token_file"/);
    assert.match(applyStep, /trap cleanup EXIT/);
    assert.match(applyStep, /bun install --frozen-lockfile/);
    assert.match(applyStep, /pulumi up --stack preview --yes --skip-preview --config "image=\$\(params\.image\)"/);
    assert.doesNotMatch(applyStep, /(?:curl|jq|jwt|auth\/kubernetes\/login|export BAO_TOKEN)/i);
    assert.doesNotMatch(outsideDeployTask, /(?:openbao-ci-jwt|openbao-ci-session|openbao-kubernetes-login|audience: openbao-pulumi-admin-v1)/);
    assert.doesNotMatch(pipeline, /openbao-pulumi-credentials/);
    assert.doesNotMatch(pipeline, /(?:uv lock --check|python -m unittest discover)/);
    assert.doesNotMatch(deployTask, /(?:results\.|\$\(results\.|secretName: openbao)/);
    assert.doesNotMatch(pipeline, /(?:ssh-keyscan|ssh-keygen|pyinfra\s|deploys\/entrypoints)/);
  });

  test("defaults machine SSH egress to empty and rejects invalid CIDRs", async () => {
    const baseConfig = allConfig();
    try {
      setAllConfig({
        ...baseConfig,
        "homelab-mcp:deployMachineSshEgressCidrs": "[]",
      });
      const resourceStart = resources.length;
      await import(`./index.ts?empty-machine-ssh-egress=${Date.now()}`);
      await pulumi.runtime.disconnect();
      const emptyPolicy = resources
        .slice(resourceStart)
        .find((candidate) => candidate.name === "homelab-mcp-egress");
      assert.ok(emptyPolicy);
      assert.equal(
        (emptyPolicy.inputs.spec as any).egress.some((rule: any) =>
          rule.ports?.some((port: any) => port.port === 22),
        ),
        false,
      );

      setAllConfig({
        ...baseConfig,
        "homelab-mcp:deployMachineSshEgressCidrs": '["172.16.0.0/16x"]',
      });
      await assert.rejects(
        import(`./index.ts?invalid-machine-ssh-egress=${Date.now()}`),
        /deployMachineSshEgressCidrs must contain valid IPv4 CIDRs/,
      );
    } finally {
      setAllConfig(baseConfig);
    }
  });
});

function resource(type: string, name: string): ResourceRecord {
  const match = [...resources].reverse().find(
    (candidate) => candidate.type === type && candidate.name === name,
  );
  if (!match) throw new Error(`missing mocked resource ${type}::${name}`);
  return match;
}

function resourceByName(name: string): ResourceRecord {
  const match = [...resources]
    .reverse()
    .find((candidate) => candidate.name === name);
  if (!match) throw new Error(`missing mocked resource ${name}`);
  return match;
}

function environment(): Record<string, unknown> {
  const appSecret = resource("kubernetes:core/v1:Secret", "homelab-mcp-app");
  assert.ok(isSecret(appSecret.inputs.stringData));
  return unwrapSecrets(appSecret.inputs.stringData) as Record<string, unknown>;
}

function isSecret(value: unknown): boolean {
  return Boolean(
    value &&
      typeof value === "object" &&
      Object.keys(value as object).some((key) => key.startsWith("4dabf181")),
  );
}

function unwrapSecrets(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(unwrapSecrets);
  if (!value || typeof value !== "object") return value;
  const record = value as Record<string, unknown>;
  if (isSecret(record) && "value" in record) return unwrapSecrets(record.value);
  return Object.fromEntries(
    Object.entries(record).map(([key, entry]) => [key, unwrapSecrets(entry)]),
  );
}

function stackFile(stack: "preview" | "prod"): string {
  return readFileSync(join(__dirname, `Pulumi.${stack}.yaml`), "utf8");
}

function restoreEnv(name: string, value: string | undefined): void {
  if (value === undefined) delete process.env[name];
  else process.env[name] = value;
}
