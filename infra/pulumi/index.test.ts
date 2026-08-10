import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { after, before, describe, test } from "node:test";
import * as pulumi from "@pulumi/pulumi";
import {
  requireImmutableImage,
  validateHttpsOrigin,
  validateWrappingKeyVersions,
} from "./policy";

interface ResourceRecord {
  type: string;
  name: string;
  inputs: Record<string, unknown>;
}

const resources: ResourceRecord[] = [];
const calls: ResourceRecord[] = [];
const previousConfig = process.env.PULUMI_CONFIG;
const previousGrafanaUrl = process.env.GRAFANA_URL;
const previousGrafanaAuth = process.env.GRAFANA_AUTH;
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
  });
  process.env.GRAFANA_URL = "https://grafana.example.test";
  process.env.GRAFANA_AUTH = "bootstrap:test-password";

  pulumi.runtime.setMocks(
    {
      newResource: (args) => {
        resources.push({
          type: args.type,
          name: args.name,
          inputs: args.inputs,
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
        if (args.name === "homelab-mcp-backups-generated-config") {
          outputs.data = { BUCKET_NAME: "test-backup-bucket" };
        }
        if (args.name === "homelab-mcp-postgres-generated-app") {
          outputs.data = {
            username: Buffer.from("app").toString("base64"),
            password: Buffer.from("password").toString("base64"),
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
    assert.equal(app.HOMELAB_MCP_OAUTH_REQUIRED_SCOPE, "mcp:use");
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
      (entry) => entry.type !== "kubernetes:core/v1:Secret",
    )) {
      assert.doesNotMatch(
        JSON.stringify(candidate.inputs),
        /HOMELAB_MCP_(?:DATABASE_URL|OIDC_CLIENT_SECRET|GRAFANA_TOKEN)/,
      );
      assert.doesNotMatch(JSON.stringify(candidate.inputs), /test-grafana-token/);
    }
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
      "homelab-mcp-app,homelab-mcp-oauth-wrapping-keys",
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
        ports: [
          { port: 443, protocol: "TCP" },
          { port: 4040, protocol: "TCP" },
          { port: 4318, protocol: "TCP" },
        ],
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
    ]);
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
