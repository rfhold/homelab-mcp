import * as authentik from "@pulumi/authentik";
import * as k8s from "@pulumi/kubernetes";
import * as pulumi from "@pulumi/pulumi";
import * as random from "@pulumi/random";
import * as tls from "@pulumi/tls";
import * as vault from "@pulumi/vault";
import * as grafana from "@pulumiverse/grafana";
import { createHash } from "node:crypto";
import { OAuthApplication } from "./authentik";
import {
  openBaoHttpsEgressRules,
  requireImmutableImage,
  validateCephClusters,
  validateHttpsOrigin,
  validateCidrs,
  validateKubernetesClusters,
  validateOpenBaoSegment,
  validateOpenBaoStack,
  validatePort,
  validateWrappingKeyVersions,
} from "./policy";

function requireEnv(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is required`);
  return value;
}

function optionalEnv(name: string): string | undefined {
  const value = process.env[name];
  return value === undefined || value.trim() === "" ? undefined : value;
}

const config = new pulumi.Config();
const namespaceName = config.require("namespace");
const hostname = config.require("hostname");
const displayName = config.require("displayName");
const slug = config.require("slug");
const image = requireImmutableImage(config.require("image"));
const authentikBaseUrl = validateHttpsOrigin(
  config.require("authentikBaseUrl"),
  "authentikBaseUrl",
);
const protectData = config.requireBoolean("protectData");
const storageClass = config.require("storageClass");
const databaseStorageSize = config.require("databaseStorageSize");
const backupStorageClass = config.require("backupStorageClass");
const backupEndpoint = validateHttpsOrigin(
  config.require("backupEndpoint"),
  "backupEndpoint",
);
const backupRetention = config.require("backupRetention");
const backupSchedule = config.require("backupSchedule");
const kubernetesClusters = validateKubernetesClusters(
  config.requireObject<unknown>("kubernetesClusters"),
);
const cephClusters = validateCephClusters(
  config.requireObject<unknown>("cephClusters"),
);
const accessTokenTtl = config.require("mcpOAuthAccessTokenTtl");
const refreshTokenTtl = config.require("mcpOAuthRefreshTokenTtl");
const refreshFamilyTtl = config.require("mcpOAuthRefreshFamilyTtl");
const codeTtl = config.require("mcpOAuthCodeTtl");
const mcpOAuthCimdTrustedPrivateOrigins = (
  config.getObject<string[]>("mcpOAuthCimdTrustedPrivateOrigins") ?? []
).map((origin) =>
  validateHttpsOrigin(origin, "mcpOAuthCimdTrustedPrivateOrigins"),
);
const wrappingKeyVersions = config.requireObject<string[]>(
  "mcpOAuthWrappingKeyVersions",
);
const activeWrappingKeyVersion = config.require(
  "mcpOAuthActiveWrappingKeyVersion",
);
validateWrappingKeyVersions(wrappingKeyVersions, activeWrappingKeyVersion);
const openbaoEnabled = config.requireBoolean("openbaoEnabled");
const openbaoUrl = validateHttpsOrigin(config.require("openbaoUrl"), "openbaoUrl");
const openbaoCreateSshMount = config.requireBoolean("openbaoCreateSshMount");
const openbaoKubernetesAuthMount = validateOpenBaoSegment(config.require("openbaoKubernetesAuthMount"), "openbaoKubernetesAuthMount");
const openbaoKubernetesRole = validateOpenBaoSegment(config.require("openbaoKubernetesRole"), "openbaoKubernetesRole");
const openbaoSshMount = validateOpenBaoSegment(config.require("openbaoSshMount"), "openbaoSshMount");
const openbaoSshRole = validateOpenBaoSegment(config.require("openbaoSshRole"), "openbaoSshRole");
const openbaoAudience = validateHttpsOrigin(config.require("openbaoAudience"), "openbaoAudience");
const openbaoEndpointCidrs = validateCidrs(config.requireObject<unknown>("openbaoEndpointCidrs"), "openbaoEndpointCidrs");
const openbaoPort = validatePort(config.requireNumber("openbaoPort"), "openbaoPort");
const configuredDeployMachineSshEgressCidrs =
  config.getObject<unknown>("deployMachineSshEgressCidrs") ?? [];
const deployMachineSshEgressCidrs =
  Array.isArray(configuredDeployMachineSshEgressCidrs) &&
  configuredDeployMachineSshEgressCidrs.length === 0
    ? []
    : validateCidrs(
        configuredDeployMachineSshEgressCidrs,
        "deployMachineSshEgressCidrs",
      );
validateOpenBaoStack(pulumi.getStack(), openbaoEnabled, openbaoCreateSshMount);

const kubernetesProviders = new Map(
  kubernetesClusters.map((cluster) => [
    cluster.name,
    new k8s.Provider(`homelab-mcp-${cluster.name}`, {
      context: cluster.context,
    }),
  ]),
);
const pantheonCluster = kubernetesClusters.find(
  (cluster) => cluster.name === "pantheon" && cluster.context === "pantheon",
);
if (!pantheonCluster) {
  throw new Error(
    "kubernetesClusters must include the Pantheon hosting cluster and context",
  );
}
const pantheonProvider = kubernetesProviders.get("pantheon")!;

const grafanaUrl = validateHttpsOrigin(requireEnv("GRAFANA_URL"), "GRAFANA_URL");
const grafanaProvider = new grafana.Provider("homelab-mcp-grafana", {
  url: grafanaUrl,
  auth: pulumi.secret(requireEnv("GRAFANA_AUTH")),
});
const forgejoToken = new pulumi.Stash("homelab-mcp-forgejo-token", {
  input: pulumi.secret(optionalEnv("FORGEJO_HOLDENITDOWN_TOKEN")),
});
const pacIncomingSecret = new pulumi.Stash("homelab-mcp-pac-incoming-secret", {
  input: pulumi.secret(optionalEnv("PAC_INCOMING_SECRET")),
});
const cephCredentials = cephClusters.map((cluster) => {
  const environmentPrefix = cluster.name.toUpperCase().replace(/-/g, "_");
  const username = new pulumi.Stash(
    `homelab-mcp-ceph-${cluster.name}-username`,
    {
      input: pulumi.secret(
        optionalEnv(`CEPH_DASHBOARD_${environmentPrefix}_USERNAME`),
      ),
    },
  );
  const password = new pulumi.Stash(
    `homelab-mcp-ceph-${cluster.name}-password`,
    {
      input: pulumi.secret(
        optionalEnv(`CEPH_DASHBOARD_${environmentPrefix}_PASSWORD`),
      ),
    },
  );
  return {
    cluster,
    username,
    password,
    usernameKey: `HOMELAB_MCP_CEPH_${environmentPrefix}_USERNAME`,
    passwordKey: `HOMELAB_MCP_CEPH_${environmentPrefix}_PASSWORD`,
  };
});

const labels = {
  "app.kubernetes.io/name": "homelab-mcp",
  "app.kubernetes.io/instance": slug,
  "app.kubernetes.io/part-of": "homelab-mcp",
  "app.kubernetes.io/managed-by": "pulumi",
};
const workloadLabels = { ...labels, "app.kubernetes.io/component": "server" };
const deploymentEnvironment = pulumi.getStack();
const tektonNamespace = "pipelines-as-code";
const forgejoOrigin = "https://git.holdenitdown.net";
const pacUrl =
  "http://pipelines-as-code-controller.pipelines-as-code.svc.cluster.local:8080";
const publicUrl = `https://${hostname}`;
const browserCallback = `${publicUrl}/oidc/callback`;
const mcpIssuer = `${publicUrl}/oauth`;
const mcpResource = `${publicUrl}/mcp`;
const wrappingKeyMountPath = "/var/run/secrets/homelab-mcp/oauth";
const wrappingKeyFile = `${wrappingKeyMountPath}/keyring.json`;
const postgresTrustMountPath = "/var/run/secrets/homelab-mcp/postgres";
const postgresCaFile = `${postgresTrustMountPath}/ca.crt`;
const openbaoJwtMountPath = "/var/run/secrets/homelab-mcp/openbao";
const openbaoJwtFile = `${openbaoJwtMountPath}/token`;
const deployRoot = "/opt/homelab-mcp";
const deployTempRoot = "/var/run/homelab-mcp/deploy";

const openbaoResources: pulumi.Resource[] = [];
if (openbaoEnabled) {
  const provider = new vault.Provider("homelab-mcp-openbao", {
    address: openbaoUrl,
    skipChildToken: true,
  });
  const sshMount = openbaoCreateSshMount
    ? new vault.Mount("homelab-mcp-openbao-ssh", {
        type: "ssh",
        path: openbaoSshMount,
        defaultLeaseTtlSeconds: 900,
        maxLeaseTtlSeconds: 900,
      }, { provider })
    : undefined;
  const ca = new vault.ssh.SecretBackendCa("homelab-mcp-openbao-ssh-ca", {
    backend: openbaoSshMount,
    generateSigningKey: true,
    keyType: "ed25519",
  }, { dependsOn: sshMount ? [sshMount] : [], provider });
  const sshRole = new vault.ssh.SecretBackendRole("homelab-mcp-openbao-ssh-role", {
    backend: openbaoSshMount,
    name: openbaoSshRole,
    keyType: "ca",
    allowUserCertificates: true,
    allowHostCertificates: false,
    allowUserKeyIds: false,
    allowedUsers: "homelab",
    defaultUser: "homelab",
    ttl: "15m",
    maxTtl: "15m",
  }, { dependsOn: [ca], provider });
  const policy = new vault.Policy("homelab-mcp-openbao", {
    name: openbaoKubernetesRole,
    policy: `path "${openbaoSshMount}/sign/${openbaoSshRole}" {\n  capabilities = ["update"]\n}\n`,
  }, { provider });
  const role = new vault.kubernetes.AuthBackendRole("homelab-mcp-openbao-kubernetes-role", {
    backend: openbaoKubernetesAuthMount,
    roleName: openbaoKubernetesRole,
    audience: openbaoAudience,
    boundServiceAccountNames: ["homelab-mcp"],
    boundServiceAccountNamespaces: [namespaceName],
    tokenPolicies: [policy.name],
    tokenNoDefaultPolicy: true,
    tokenTtl: 900,
    tokenMaxTtl: 900,
    tokenExplicitMaxTtl: 900,
    tokenType: "service",
  }, { dependsOn: [policy], provider });
  openbaoResources.push(provider, ...(sshMount ? [sshMount] : []), ca, sshRole, policy, role);
}

const namespace = new k8s.core.v1.Namespace(
  "homelab-mcp-namespace",
  { metadata: { name: namespaceName, labels } },
  { provider: pantheonProvider },
);

const appServiceAccount = new k8s.core.v1.ServiceAccount(
  "homelab-mcp",
  {
    metadata: {
      name: "homelab-mcp",
      namespace: namespace.metadata.name,
      labels,
    },
    automountServiceAccountToken: false,
  },
  { dependsOn: [namespace], provider: pantheonProvider },
);

const tektonRole = new k8s.rbac.v1.Role(
  "homelab-mcp-tekton",
  {
    metadata: {
      name: "homelab-mcp",
      namespace: tektonNamespace,
      labels,
    },
    rules: [
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
      {
        apiGroups: [""],
        resources: ["pods"],
        verbs: ["get"],
      },
      {
        apiGroups: [""],
        resources: ["pods/log"],
        verbs: ["get"],
      },
    ],
  },
  { provider: pantheonProvider },
);

new k8s.rbac.v1.RoleBinding(
  "homelab-mcp-tekton",
  {
    metadata: {
      name: "homelab-mcp",
      namespace: tektonNamespace,
      labels,
      annotations: { "pulumi.com/skipAwait": "true" },
    },
    roleRef: {
      apiGroup: "rbac.authorization.k8s.io",
      kind: "Role",
      name: tektonRole.metadata.name,
    },
    subjects: [
      {
        kind: "ServiceAccount",
        name: appServiceAccount.metadata.name,
        namespace: namespace.metadata.name,
      },
    ],
  },
  {
    dependsOn: [appServiceAccount, tektonRole],
    provider: pantheonProvider,
  },
);

const kubernetesReadRules: k8s.types.input.rbac.v1.PolicyRule[] = [
  {
    nonResourceURLs: [
      "/api",
      "/apis",
      "/version",
      "/api/v1",
      "/apis/apps/v1",
      "/apis/autoscaling/v2",
      "/apis/batch/v1",
      "/apis/ceph.rook.io/v1",
      "/apis/cert-manager.io/v1",
      "/apis/discovery.k8s.io/v1",
      "/apis/events.k8s.io/v1",
      "/apis/gateway.networking.k8s.io/v1",
      "/apis/kafka.strimzi.io/v1beta2",
      "/apis/metrics.k8s.io/v1beta1",
      "/apis/networking.k8s.io/v1",
      "/apis/policy/v1",
      "/apis/postgresql.cnpg.io/v1",
      "/apis/storage.k8s.io/v1",
      "/apis/velero.io/v1",
    ],
    verbs: ["get"],
  },
  {
    apiGroups: [""],
    resources: [
      "namespaces",
      "nodes",
      "pods",
      "services",
      "persistentvolumeclaims",
    ],
    verbs: ["get", "list"],
  },
  {
    apiGroups: ["events.k8s.io"],
    resources: ["events"],
    verbs: ["get", "list"],
  },
  {
    apiGroups: ["apps"],
    resources: ["deployments", "statefulsets", "daemonsets", "replicasets"],
    verbs: ["get", "list"],
  },
  {
    apiGroups: ["batch"],
    resources: ["jobs", "cronjobs"],
    verbs: ["get", "list"],
  },
  {
    apiGroups: ["metrics.k8s.io"],
    resources: ["pods", "nodes"],
    verbs: ["get", "list"],
  },
  {
    apiGroups: ["discovery.k8s.io"],
    resources: ["endpointslices"],
    verbs: ["get", "list"],
  },
  {
    apiGroups: ["networking.k8s.io"],
    resources: ["ingresses", "networkpolicies"],
    verbs: ["get", "list"],
  },
  {
    apiGroups: ["gateway.networking.k8s.io"],
    resources: ["gatewayclasses", "gateways", "httproutes"],
    verbs: ["get", "list"],
  },
  {
    apiGroups: ["autoscaling"],
    resources: ["horizontalpodautoscalers"],
    verbs: ["get", "list"],
  },
  {
    apiGroups: ["policy"],
    resources: ["poddisruptionbudgets"],
    verbs: ["get", "list"],
  },
  {
    apiGroups: ["storage.k8s.io"],
    resources: ["storageclasses"],
    verbs: ["get", "list"],
  },
  {
    apiGroups: ["cert-manager.io"],
    resources: ["certificates", "clusterissuers"],
    verbs: ["get", "list"],
  },
  {
    apiGroups: ["postgresql.cnpg.io"],
    resources: ["clusters"],
    verbs: ["get", "list"],
  },
  {
    apiGroups: ["kafka.strimzi.io"],
    resources: ["kafkas", "kafkanodepools", "kafkatopics"],
    verbs: ["get", "list"],
  },
  {
    apiGroups: ["ceph.rook.io"],
    resources: [
      "cephclusters",
      "cephfilesystems",
      "cephblockpools",
      "cephobjectstores",
    ],
    verbs: ["get", "list"],
  },
  {
    apiGroups: ["velero.io"],
    resources: ["backups", "schedules", "backupstoragelocations"],
    verbs: ["get", "list"],
  },
];
const kubernetesWriteRules: k8s.types.input.rbac.v1.PolicyRule[] = [
  {
    apiGroups: ["apps"],
    resources: ["deployments", "statefulsets", "daemonsets"],
    verbs: ["patch"],
  },
  {
    apiGroups: ["apps"],
    resources: ["deployments/scale", "statefulsets/scale"],
    verbs: ["get", "patch"],
  },
  {
    apiGroups: ["batch"],
    resources: ["cronjobs"],
    verbs: ["patch"],
  },
  { apiGroups: ["batch"], resources: ["jobs"], verbs: ["create"] },
  { apiGroups: [""], resources: ["pods"], verbs: ["delete"] },
];

const runtimeIdentities = kubernetesClusters.map((cluster) => {
  const provider = kubernetesProviders.get(cluster.name)!;
  const targetNamespace =
    cluster.name === "pantheon"
      ? namespace
      : new k8s.core.v1.Namespace(
          `homelab-mcp-runtime-namespace-${cluster.name}`,
          { metadata: { name: namespaceName, labels } },
          { provider },
        );
  const identityName = `${slug}-kubernetes-runtime`;
  const account = new k8s.core.v1.ServiceAccount(
    `homelab-mcp-kubernetes-runtime-${cluster.name}`,
    {
      metadata: {
        name: identityName,
        namespace: targetNamespace.metadata.name,
        labels,
      },
      automountServiceAccountToken: false,
    },
    { dependsOn: [targetNamespace], provider },
  );
  const role = new k8s.rbac.v1.ClusterRole(
    `homelab-mcp-kubernetes-runtime-${cluster.name}`,
    {
      metadata: { name: identityName, labels },
      rules: [...kubernetesReadRules, ...kubernetesWriteRules],
    },
    { provider },
  );
  new k8s.rbac.v1.ClusterRoleBinding(
    `homelab-mcp-kubernetes-runtime-${cluster.name}`,
    {
      metadata: { name: identityName, labels },
      roleRef: {
        apiGroup: "rbac.authorization.k8s.io",
        kind: "ClusterRole",
        name: role.metadata.name,
      },
      subjects: [
        {
          kind: "ServiceAccount",
          name: account.metadata.name,
          namespace: targetNamespace.metadata.name,
        },
      ],
    },
    { dependsOn: [account, role], provider },
  );
  const tokenSecret = new k8s.core.v1.Secret(
    `homelab-mcp-kubernetes-runtime-token-${cluster.name}`,
    {
      metadata: {
        name: `${identityName}-token`,
        namespace: targetNamespace.metadata.name,
        labels,
        annotations: {
          "kubernetes.io/service-account.name": identityName,
          "pulumi.com/waitFor": "jsonpath={.data.token}",
        },
      },
      type: "kubernetes.io/service-account-token",
    },
    { dependsOn: [account], provider },
  );
  const issuedToken = k8s.core.v1.Secret.get(
    `homelab-mcp-kubernetes-runtime-token-read-${cluster.name}`,
    pulumi.interpolate`${targetNamespace.metadata.name}/${tokenSecret.metadata.name}`,
    { dependsOn: [tokenSecret], provider },
  );
  const credentials = pulumi.secret(
    issuedToken.data.apply((data) => {
      const token = data?.token;
      const ca = data?.["ca.crt"];
      if (!token || !ca) {
        throw new Error(`runtime token for ${cluster.name} is not populated`);
      }
      return {
        token: Buffer.from(token, "base64").toString("utf8"),
        certificateAuthorityData: ca,
      };
    }),
  );
  return { cluster, credentials, tokenSecret };
});

const kubernetesKubeconfigPath =
  "/var/run/secrets/homelab-mcp/kubernetes/kubeconfig";
const runtimeKubeconfig = pulumi.secret(
  pulumi
    .all(runtimeIdentities.map((identity) => identity.credentials))
    .apply((credentials) =>
      JSON.stringify({
        apiVersion: "v1",
        kind: "Config",
        clusters: kubernetesClusters.map((cluster, index) => ({
          name: cluster.context,
          cluster: {
            server: cluster.server,
            "certificate-authority-data":
              credentials[index].certificateAuthorityData,
          },
        })),
        users: kubernetesClusters.map((cluster, index) => ({
          name: cluster.context,
          user: { token: credentials[index].token },
        })),
        contexts: kubernetesClusters.map((cluster) => ({
          name: cluster.context,
          context: { cluster: cluster.context, user: cluster.context },
        })),
        "current-context": pantheonCluster.context,
      }),
    ),
);
const runtimeKubeconfigSecret = new k8s.core.v1.Secret(
  "homelab-mcp-kubernetes-runtime-kubeconfig",
  {
    metadata: {
      name: "homelab-mcp-kubernetes-runtime-kubeconfig",
      namespace: namespace.metadata.name,
      labels,
    },
    type: "Opaque",
    stringData: { kubeconfig: runtimeKubeconfig },
  },
  {
    dependsOn: runtimeIdentities.map((identity) => identity.tokenSecret),
    provider: pantheonProvider,
  },
);

const backupBucket = new k8s.apiextensions.CustomResource(
  "homelab-mcp-backups-bucket",
  {
    apiVersion: "objectbucket.io/v1alpha1",
    kind: "ObjectBucketClaim",
    metadata: {
      name: "homelab-mcp-backups",
      namespace: namespace.metadata.name,
      labels,
    },
    spec: {
      storageClassName: backupStorageClass,
      generateBucketName: `${slug}-backups`,
    },
  },
  {
    dependsOn: [namespace],
    protect: protectData,
    provider: pantheonProvider,
  },
);
const backupConfig = pulumi
  .all([namespace.metadata.name, backupBucket.id])
  .apply(([resolvedNamespace]) =>
    k8s.core.v1.ConfigMap.get(
      "homelab-mcp-backups-generated-config",
      `${resolvedNamespace}/homelab-mcp-backups`,
      { provider: pantheonProvider },
    ),
  );

const databaseCluster = new k8s.apiextensions.CustomResource(
  "homelab-mcp-postgres",
  {
    apiVersion: "postgresql.cnpg.io/v1",
    kind: "Cluster",
    metadata: {
      name: "homelab-mcp-postgres",
      namespace: namespace.metadata.name,
      labels,
    },
    spec: {
      instances: 1,
      enableSuperuserAccess: false,
      backup: {
        retentionPolicy: backupRetention,
        barmanObjectStore: {
          destinationPath: backupConfig.data.apply(
            (values) => `s3://${values.BUCKET_NAME}/database`,
          ),
          endpointURL: backupEndpoint,
          s3Credentials: {
            accessKeyId: {
              name: "homelab-mcp-backups",
              key: "AWS_ACCESS_KEY_ID",
            },
            secretAccessKey: {
              name: "homelab-mcp-backups",
              key: "AWS_SECRET_ACCESS_KEY",
            },
          },
          wal: { compression: "gzip" },
          data: { compression: "gzip" },
        },
      },
      storage: { size: databaseStorageSize, storageClass },
      resources: {
        requests: { cpu: "100m", memory: "256Mi" },
        limits: { cpu: "1", memory: "1Gi" },
      },
    },
  },
  {
    dependsOn: [backupBucket],
    protect: protectData,
    provider: pantheonProvider,
  },
);

new k8s.apiextensions.CustomResource(
  "homelab-mcp-postgres-backup",
  {
    apiVersion: "postgresql.cnpg.io/v1",
    kind: "ScheduledBackup",
    metadata: {
      name: "homelab-mcp-postgres",
      namespace: namespace.metadata.name,
      labels,
    },
    spec: {
      schedule: backupSchedule,
      backupOwnerReference: "self",
      cluster: { name: "homelab-mcp-postgres" },
      immediate: true,
      method: "barmanObjectStore",
    },
  },
  { dependsOn: [databaseCluster], provider: pantheonProvider },
);

const database = new k8s.apiextensions.CustomResource(
  "homelab-mcp-database",
  {
    apiVersion: "postgresql.cnpg.io/v1",
    kind: "Database",
    metadata: {
      name: "homelab-mcp",
      namespace: namespace.metadata.name,
      labels,
    },
    spec: {
      name: "homelab_mcp",
      owner: "app",
      cluster: { name: "homelab-mcp-postgres" },
    },
  },
  { dependsOn: [databaseCluster], provider: pantheonProvider },
);

const signingPrivateKey = new tls.PrivateKey("homelab-mcp-oidc-signing-key", {
  algorithm: "RSA",
  rsaBits: 4096,
});
const signingCertificate = new tls.SelfSignedCert(
  "homelab-mcp-oidc-signing-certificate",
  {
    privateKeyPem: signingPrivateKey.privateKeyPem,
    subject: { commonName: `${slug} OAuth signing` },
    validityPeriodHours: 87600,
    allowedUses: ["digital_signature", "key_encipherment"],
  },
);
const signingKey = new authentik.CertificateKeyPair(
  "homelab-mcp-oidc-signing-keypair",
  {
    name: `${displayName} OAuth signing key`,
    certificateData: signingCertificate.certPem,
    keyData: signingPrivateKey.privateKeyPem,
  },
);
const openid = authentik.getPropertyMappingProviderScopeOutput({
  managed: "goauthentik.io/providers/oauth2/scope-openid",
});
const profile = authentik.getPropertyMappingProviderScopeOutput({
  managed: "goauthentik.io/providers/oauth2/scope-profile",
});
const email = authentik.getPropertyMappingProviderScopeOutput({
  managed: "goauthentik.io/providers/oauth2/scope-email",
});
const browserApp = new OAuthApplication("homelab-mcp-browser", {
  displayName: `${displayName} Browser`,
  slug: `${slug}-browser`,
  issuerBaseUrl: authentikBaseUrl,
  redirectUri: browserCallback,
  signingKeyId: signingKey.id,
  propertyMappings: [openid.id, profile.id, email.id],
  launchUrl: publicUrl,
});

const grafanaServiceAccount = new grafana.oss.ServiceAccount(
  "homelab-mcp-grafana-service-account",
  { name: slug, role: "Editor" },
  { provider: grafanaProvider },
);
const grafanaToken = new grafana.oss.ServiceAccountToken(
  "homelab-mcp-grafana-token",
  {
    name: slug,
    serviceAccountId: grafanaServiceAccount.id,
    secondsToLive: 0,
  },
  { provider: grafanaProvider },
);

const cnpgAppSecret = pulumi
  .all([namespace.metadata.name, databaseCluster.id])
  .apply(([resolvedNamespace]) =>
    k8s.core.v1.Secret.get(
      "homelab-mcp-postgres-generated-app",
      `${resolvedNamespace}/homelab-mcp-postgres-app`,
      { provider: pantheonProvider },
    ),
  );
const decodeDatabaseSecret = (key: string) =>
  cnpgAppSecret.data.apply((values) =>
    Buffer.from(values[key], "base64").toString("utf8"),
  );
const databaseUrl = pulumi
  .all([decodeDatabaseSecret("username"), decodeDatabaseSecret("password")])
  .apply(
    ([username, password]) =>
      `postgresql://${encodeURIComponent(username)}:${encodeURIComponent(password)}@homelab-mcp-postgres-rw.${namespaceName}.svc:5432/homelab_mcp?sslmode=verify-full&sslrootcert=${encodeURIComponent(postgresCaFile)}`,
  );

const wrappingKeys = wrappingKeyVersions.map((version) => ({
  version,
  key: new random.RandomBytes(
    `homelab-mcp-oauth-wrapping-key-${version}`,
    { length: 32 },
  ).base64,
}));
const wrappingKeyring = pulumi.secret(
  pulumi
    .all(wrappingKeys.map(({ key }) => key))
    .apply((keys) => {
      if (keys.some((key) => typeof key !== "string" || !key)) return "";
      return JSON.stringify({
        schema_version: 1,
        active: activeWrappingKeyVersion,
        keys: wrappingKeyVersions.map((version, index) => ({
          id: version,
          key: Buffer.from(keys[index], "base64").toString("base64url"),
        })),
      });
    }),
);
const wrappingKeyChecksum = wrappingKeyring.apply((keyring) =>
  createHash("sha256").update(keyring).digest("hex"),
);
const wrappingKeySecret = new k8s.core.v1.Secret(
  "homelab-mcp-oauth-wrapping-keys",
  {
    metadata: {
      name: "homelab-mcp-oauth-wrapping-keys",
      namespace: namespace.metadata.name,
      labels,
      annotations: { "homelab-mcp.holdenitdown.net/keyring-format": "1" },
    },
    type: "Opaque",
    stringData: { "keyring.json": wrappingKeyring },
  },
  { provider: pantheonProvider },
);

const appSecret = new k8s.core.v1.Secret(
  "homelab-mcp-app",
  {
    metadata: {
      name: "homelab-mcp-app",
      namespace: namespace.metadata.name,
      labels,
    },
    stringData: {
      HOMELAB_MCP_DATABASE_URL: databaseUrl,
      HOMELAB_MCP_DATABASE_MAX_CONNECTIONS: "10",
      HOMELAB_MCP_PUBLIC_URL: publicUrl,
      HOMELAB_MCP_OIDC_ISSUER: browserApp.issuer,
      HOMELAB_MCP_OIDC_CLIENT_ID: browserApp.clientId,
      HOMELAB_MCP_OIDC_CLIENT_SECRET: browserApp.clientSecret,
      HOMELAB_MCP_OIDC_REDIRECT_URI: browserCallback,
      HOMELAB_MCP_OIDC_SCOPES: "openid profile email",
      HOMELAB_MCP_OAUTH_ISSUER: mcpIssuer,
      HOMELAB_MCP_OAUTH_RESOURCE: mcpResource,
      HOMELAB_MCP_OAUTH_REQUIRED_SCOPES:
        "mcp:use kubernetes:read kubernetes:write inventory:read inventory:write inventory:host-trust deploy:read deploy:run",
      HOMELAB_MCP_OAUTH_ACCESS_TOKEN_TTL: accessTokenTtl,
      HOMELAB_MCP_OAUTH_REFRESH_TOKEN_TTL: refreshTokenTtl,
      HOMELAB_MCP_OAUTH_REFRESH_FAMILY_TTL: refreshFamilyTtl,
      HOMELAB_MCP_OAUTH_CODE_TTL: codeTtl,
      HOMELAB_MCP_OAUTH_ALLOW_DCR: "true",
      HOMELAB_MCP_OAUTH_ALLOW_CIMD: "true",
      ...(mcpOAuthCimdTrustedPrivateOrigins.length > 0
        ? {
            HOMELAB_MCP_OAUTH_CIMD_TRUSTED_PRIVATE_ORIGINS:
              mcpOAuthCimdTrustedPrivateOrigins.join(","),
          }
        : {}),
      HOMELAB_MCP_OAUTH_ALLOW_LOOPBACK_REDIRECTS: "true",
      HOMELAB_MCP_OAUTH_WRAPPING_KEYS_FILE: wrappingKeyFile,
      HOMELAB_MCP_GRAFANA_URL: grafanaUrl,
      HOMELAB_MCP_GRAFANA_TOKEN: pulumi.secret(grafanaToken.key),
      HOMELAB_MCP_FORGEJO_ORIGIN: forgejoOrigin,
      HOMELAB_MCP_FORGEJO_TOKEN: forgejoToken.output,
      HOMELAB_MCP_TEKTON_NAMESPACE: tektonNamespace,
      HOMELAB_MCP_PAC_URL: pacUrl,
      HOMELAB_MCP_PAC_INCOMING_SECRET: pacIncomingSecret.output,
      HOMELAB_MCP_KUBECTL_PATH: "/usr/local/bin/kubectl",
      HOMELAB_MCP_KUBERNETES_CLUSTERS: JSON.stringify(
        kubernetesClusters.map((cluster) => ({
          name: cluster.name,
          kubeconfig: kubernetesKubeconfigPath,
          context: cluster.context,
          cache_dir: `/tmp/kubectl/${cluster.name}`,
        })),
      ),
      HOMELAB_MCP_CEPH_CLUSTERS: JSON.stringify(
        cephCredentials.map(
          ({ cluster, usernameKey, passwordKey }) => ({
            name: cluster.name,
            origin: cluster.origin,
            expected_major_release: cluster.expectedMajorRelease,
            username_env: usernameKey,
            password_env: passwordKey,
          }),
        ),
      ),
      HOMELAB_MCP_DEPLOY_ROOT: deployRoot,
      HOMELAB_MCP_DEPLOY_CATALOG: `${deployRoot}/deploys/catalog.json`,
      HOMELAB_MCP_DEPLOY_UV_EXECUTABLE: "/usr/local/bin/uv",
      HOMELAB_MCP_DEPLOY_SSH_KEYGEN_EXECUTABLE: "/usr/bin/ssh-keygen",
      HOMELAB_MCP_DEPLOY_TEMP_ROOT: deployTempRoot,
      HOMELAB_MCP_OPENBAO_URL: openbaoUrl,
      HOMELAB_MCP_OPENBAO_KUBERNETES_AUTH_MOUNT: openbaoKubernetesAuthMount,
      HOMELAB_MCP_OPENBAO_KUBERNETES_ROLE: openbaoKubernetesRole,
      HOMELAB_MCP_OPENBAO_SSH_MOUNT: openbaoSshMount,
      HOMELAB_MCP_OPENBAO_SSH_ROLE: openbaoSshRole,
      HOMELAB_MCP_OPENBAO_JWT_PATH: openbaoJwtFile,
      HOMELAB_MCP_OPENBAO_REQUEST_TIMEOUT_MS: "5000",
      ...Object.fromEntries(
        cephCredentials.flatMap(
          ({ username, password, usernameKey, passwordKey }) => [
            [usernameKey, username.output],
            [passwordKey, password.output],
          ],
        ),
      ),
      HOMELAB_MCP_DEPLOYMENT_ENVIRONMENT: deploymentEnvironment,
      HOMELAB_MCP_SERVICE_NAMESPACE: "homelab",
      HOMELAB_MCP_PYROSCOPE_URL: "https://telemetry.holdenitdown.net:4040",
      OTEL_EXPORTER_OTLP_ENDPOINT: "https://telemetry.holdenitdown.net:4318",
      OTEL_EXPORTER_OTLP_PROTOCOL: "http/protobuf",
      OTEL_SERVICE_NAME: "homelab-mcp",
      OTEL_RESOURCE_ATTRIBUTES: `service.namespace=homelab,deployment.environment.name=${deploymentEnvironment}`,
    },
  },
  {
    dependsOn: [
      database,
      browserApp,
      grafanaToken,
      forgejoToken,
      pacIncomingSecret,
      ...cephCredentials.flatMap(({ username, password }) => [
        username,
        password,
      ]),
      runtimeKubeconfigSecret,
    ],
    provider: pantheonProvider,
  },
);

new k8s.apps.v1.Deployment(
  "homelab-mcp",
  {
    metadata: {
      name: "homelab-mcp",
      namespace: namespace.metadata.name,
      labels: workloadLabels,
      annotations: {
        "secret.reloader.stakater.com/reload":
          "homelab-mcp-app,homelab-mcp-oauth-wrapping-keys,homelab-mcp-kubernetes-runtime-kubeconfig",
      },
    },
    spec: {
      replicas: 1,
      strategy: { type: "Recreate" },
      selector: { matchLabels: workloadLabels },
      template: {
        metadata: {
          labels: workloadLabels,
          annotations: {
            "homelab-mcp.holdenitdown.net/wrapping-key-checksum":
              wrappingKeyChecksum,
            "resource.opentelemetry.io/service.name": "homelab-mcp",
            "resource.opentelemetry.io/service.namespace": "homelab",
            "resource.opentelemetry.io/deployment.environment.name":
              deploymentEnvironment,
          },
        },
        spec: {
          automountServiceAccountToken: false,
          serviceAccountName: appServiceAccount.metadata.name,
          securityContext: {
            runAsNonRoot: true,
            runAsUser: 65532,
            runAsGroup: 65532,
            fsGroup: 65532,
            fsGroupChangePolicy: "OnRootMismatch",
            seccompProfile: { type: "RuntimeDefault" },
          },
          terminationGracePeriodSeconds: 30,
          containers: [
            {
              name: "homelab-mcp",
              image,
              imagePullPolicy: "IfNotPresent",
              ports: [
                { name: "http", containerPort: 14333, protocol: "TCP" },
              ],
              envFrom: [{ secretRef: { name: appSecret.metadata.name } }],
              env: [
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
              ],
              securityContext: {
                runAsNonRoot: true,
                allowPrivilegeEscalation: false,
                readOnlyRootFilesystem: true,
                capabilities: { drop: ["ALL"] },
              },
              resources: {
                requests: { cpu: "50m", memory: "64Mi" },
                limits: { cpu: "500m", memory: "256Mi" },
              },
              startupProbe: {
                httpGet: { path: "/health", port: "http" },
                periodSeconds: 2,
                failureThreshold: 60,
              },
              readinessProbe: {
                httpGet: { path: "/ready", port: "http" },
                periodSeconds: 5,
                failureThreshold: 3,
              },
              livenessProbe: {
                httpGet: { path: "/health", port: "http" },
                periodSeconds: 10,
                failureThreshold: 3,
              },
              volumeMounts: [
                { name: "tmp", mountPath: "/tmp" },
                {
                  name: "oauth-wrapping-keys",
                  mountPath: wrappingKeyMountPath,
                  readOnly: true,
                },
                {
                  name: "postgres-ca",
                  mountPath: postgresTrustMountPath,
                  readOnly: true,
                },
                {
                  name: "kubernetes-runtime-kubeconfig",
                  mountPath: "/var/run/secrets/homelab-mcp/kubernetes",
                  readOnly: true,
                },
                {
                  name: "openbao-jwt",
                  mountPath: openbaoJwtMountPath,
                  readOnly: true,
                },
                {
                  name: "deploy-credentials",
                  mountPath: deployTempRoot,
                },
                {
                  name: "kube-api-access",
                  mountPath: "/var/run/secrets/kubernetes.io/serviceaccount",
                  readOnly: true,
                },
              ],
            },
          ],
          volumes: [
            { name: "tmp", emptyDir: { sizeLimit: "64Mi" } },
            {
              name: "deploy-credentials",
              emptyDir: { medium: "Memory", sizeLimit: "16Mi" },
            },
            {
              name: "openbao-jwt",
              projected: {
                defaultMode: 0o440,
                sources: [{
                  serviceAccountToken: {
                    audience: openbaoAudience,
                    expirationSeconds: 600,
                    path: "token",
                  },
                }],
              },
            },
            {
              name: "oauth-wrapping-keys",
              secret: {
                secretName: wrappingKeySecret.metadata.name,
                defaultMode: 0o440,
                items: [
                  { key: "keyring.json", path: "keyring.json", mode: 0o440 },
                ],
              },
            },
            {
              name: "postgres-ca",
              secret: {
                secretName: "homelab-mcp-postgres-ca",
                defaultMode: 0o444,
                items: [{ key: "ca.crt", path: "ca.crt", mode: 0o444 }],
              },
            },
            {
              name: "kubernetes-runtime-kubeconfig",
              secret: {
                secretName: runtimeKubeconfigSecret.metadata.name,
                defaultMode: 0o440,
                items: [
                  { key: "kubeconfig", path: "kubeconfig", mode: 0o440 },
                ],
              },
            },
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
          ],
        },
      },
    },
  },
  {
    dependsOn: [
      appSecret,
      wrappingKeySecret,
      runtimeKubeconfigSecret,
      appServiceAccount,
      ...openbaoResources,
    ],
    provider: pantheonProvider,
  },
);

new k8s.networking.v1.NetworkPolicy(
  "homelab-mcp-egress",
  {
    metadata: {
      name: "homelab-mcp-egress",
      namespace: namespace.metadata.name,
      labels,
    },
    spec: {
      podSelector: { matchLabels: workloadLabels },
      policyTypes: ["Egress"],
      egress: [
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
        ...openBaoHttpsEgressRules(
          openbaoEnabled,
          openbaoEndpointCidrs,
          openbaoPort,
        ),
        ...deployMachineSshEgressCidrs.map((cidr) => ({
          to: [{ ipBlock: { cidr } }],
          ports: [{ port: 22, protocol: "TCP" as const }],
        })),
        {
          ports: [
            { port: 4040, protocol: "TCP" },
            { port: 4318, protocol: "TCP" },
          ],
        },
        ...kubernetesClusters.flatMap((cluster) =>
          cluster.apiServerEndpointCidrs.map((cidr) => ({
            to: [{ ipBlock: { cidr } }],
            ports: [{ port: cluster.apiServerPort, protocol: "TCP" as const }],
          })),
        ),
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
                  "kubernetes.io/metadata.name": tektonNamespace,
                },
              },
            },
          ],
          ports: [{ port: 8080, protocol: "TCP" }],
        },
      ],
    },
  },
  { provider: pantheonProvider },
);

const service = new k8s.core.v1.Service(
  "homelab-mcp",
  {
    metadata: {
      name: "homelab-mcp",
      namespace: namespace.metadata.name,
      labels,
    },
    spec: {
      type: "ClusterIP",
      selector: workloadLabels,
      ports: [
        {
          name: "http",
          port: 14333,
          targetPort: "http",
        },
      ],
    },
  },
  { provider: pantheonProvider },
);

new k8s.apiextensions.CustomResource(
  "homelab-mcp-route",
  {
    apiVersion: "gateway.networking.k8s.io/v1",
    kind: "HTTPRoute",
    metadata: {
      name: "homelab-mcp",
      namespace: namespace.metadata.name,
      labels,
    },
    spec: {
      parentRefs: [
        {
          group: "gateway.networking.k8s.io",
          kind: "Gateway",
          name: "default-gateway",
          namespace: "ingress",
        },
      ],
      hostnames: [hostname],
      rules: [
        {
          matches: [{ path: { type: "PathPrefix", value: "/" } }],
          backendRefs: [{ name: service.metadata.name, port: 14333 }],
          timeouts: { request: "0s" },
        },
      ],
    },
  },
  { provider: pantheonProvider },
);

export const namespaceNameOutput = namespace.metadata.name;
export const publicUrlOutput = publicUrl;
export const mcpOAuthIssuer = mcpIssuer;
export const mcpOAuthResource = mcpResource;
export const mcpOAuthMetadataUrl = `${publicUrl}/.well-known/oauth-authorization-server/oauth`;
export const mcpOAuthJwksUrl = `${mcpIssuer}/jwks.json`;
