import { isIP } from "node:net";

export function requireImmutableImage(image: string): string {
  if (!/^[^\s@]+@sha256:[a-f0-9]{64}$/.test(image)) {
    throw new Error("image must be an immutable sha256 digest reference");
  }
  return image;
}

export function validateWrappingKeyVersions(
  versions: string[],
  activeVersion: string,
): void {
  if (
    versions.length === 0 ||
    new Set(versions).size !== versions.length ||
    !versions.includes(activeVersion) ||
    versions.some((version) => !/^[a-z0-9][a-z0-9-]{0,62}$/.test(version))
  ) {
    throw new Error(
      "mcpOAuthWrappingKeyVersions must be unique DNS labels and contain mcpOAuthActiveWrappingKeyVersion",
    );
  }
}

export function validateHttpsOrigin(value: string, name: string): string {
  const normalized = value.replace(/\/$/, "");
  const url = new URL(normalized);
  if (
    url.protocol !== "https:" ||
    url.username !== "" ||
    url.password !== "" ||
    url.pathname !== "/" ||
    url.search !== "" ||
    url.hash !== ""
  ) {
    throw new Error(
      `${name} must be an HTTPS origin without credentials, path, query, or fragment`,
    );
  }
  return normalized;
}

export function validateOpenBaoSegment(value: string, name: string): string {
  if (!/^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$/.test(value)) {
    throw new Error(`${name} must be a single safe OpenBao path segment`);
  }
  return value;
}

export function validateCidrs(values: unknown, name: string): string[] {
  if (!Array.isArray(values) || values.length === 0 || values.length > 16) {
    throw new Error(`${name} must contain between 1 and 16 CIDRs`);
  }
  for (const value of values) {
    if (typeof value !== "string") throw new Error(`${name} must contain CIDRs`);
    const [address, prefix, extra] = value.split("/");
    const family = isIP(address);
    const bits = Number(prefix);
    if (
      extra !== undefined ||
      family !== 4 ||
      !/^\d+$/.test(prefix ?? "") ||
      bits > 32
    ) {
      throw new Error(`${name} must contain valid IPv4 CIDRs`);
    }
  }
  return [...values];
}

export function validatePort(value: number, name: string): number {
  if (!Number.isInteger(value) || value < 1 || value > 65_535) {
    throw new Error(`${name} must be between 1 and 65535`);
  }
  return value;
}

export function validateOpenBaoStack(
  stack: string,
  enabled: boolean,
  createSshMount: boolean,
): boolean {
  const preview = stack === "preview" || stack === "test";
  if (enabled !== preview || (!enabled && createSshMount)) {
    throw new Error("OpenBao resources are preview-only and must be disabled in production");
  }
  return enabled;
}

export function openBaoHttpsEgressRules(
  enabled: boolean,
  endpointCidrs: string[],
  port: number,
) {
  return [
    {
      to: [{
        ipBlock: {
          cidr: "0.0.0.0/0",
          ...(enabled ? { except: endpointCidrs } : {}),
        },
      }],
      ports: [{ port: 443, protocol: "TCP" as const }],
    },
    ...(enabled
      ? endpointCidrs.map((cidr) => ({
          to: [{ ipBlock: { cidr } }],
          ports: [{ port, protocol: "TCP" as const }],
        }))
      : []),
  ];
}

export interface CephClusterConfig {
  name: string;
  origin: string;
  expectedMajorRelease: 19;
}

export function validateCephClusters(clusters: unknown): CephClusterConfig[] {
  if (!Array.isArray(clusters)) {
    throw new Error("cephClusters must be an array");
  }
  if (clusters.length > 32) {
    throw new Error("cephClusters must contain at most 32 entries");
  }

  const names = new Set<string>();
  return clusters.map((value, index) => {
    if (value === null || typeof value !== "object" || Array.isArray(value)) {
      throw new Error(`cephClusters.${index} must be an object`);
    }
    const cluster = value as Record<string, unknown>;
    const expectedKeys = ["expectedMajorRelease", "name", "origin"];
    if (
      Object.keys(cluster).length !== expectedKeys.length ||
      Object.keys(cluster).some((key) => !expectedKeys.includes(key))
    ) {
      throw new Error(`cephClusters.${index} contains unknown fields`);
    }
    if (
      typeof cluster.name !== "string" ||
      typeof cluster.origin !== "string" ||
      cluster.expectedMajorRelease !== 19
    ) {
      throw new Error(`cephClusters.${index} has invalid field values`);
    }
    if (!/^[a-z0-9][a-z0-9-]{0,62}$/.test(cluster.name)) {
      throw new Error("cephClusters names must be safe DNS labels");
    }
    if (names.has(cluster.name)) {
      throw new Error("cephClusters names must be unique");
    }
    names.add(cluster.name);
    return {
      name: cluster.name,
      origin: validateHttpsOrigin(
        cluster.origin,
        `cephClusters.${cluster.name}.origin`,
      ),
      expectedMajorRelease: 19,
    };
  });
}

export interface KubernetesClusterConfig {
  name: string;
  context: string;
  server: string;
  apiServerEndpointCidrs: string[];
  apiServerPort: number;
}

export function validateKubernetesClusters(
  clusters: unknown,
): KubernetesClusterConfig[] {
  if (!Array.isArray(clusters)) {
    throw new Error("kubernetesClusters must be an array");
  }
  if (clusters.length < 1 || clusters.length > 32) {
    throw new Error("kubernetesClusters must contain between 1 and 32 entries");
  }

  const names = new Set<string>();
  const contexts = new Set<string>();
  return clusters.map((value, index) => {
    if (value === null || typeof value !== "object" || Array.isArray(value)) {
      throw new Error(`kubernetesClusters.${index} must be an object`);
    }
    const cluster = value as Record<string, unknown>;
    const expectedKeys = [
      "apiServerEndpointCidrs",
      "context",
      "name",
      "server",
    ];
    if (
      Object.keys(cluster).length !== expectedKeys.length ||
      Object.keys(cluster).some((key) => !expectedKeys.includes(key))
    ) {
      throw new Error(`kubernetesClusters.${index} contains unknown fields`);
    }
    if (
      typeof cluster.name !== "string" ||
      typeof cluster.context !== "string" ||
      typeof cluster.server !== "string" ||
      !Array.isArray(cluster.apiServerEndpointCidrs) ||
      cluster.apiServerEndpointCidrs.some((cidr) => typeof cidr !== "string")
    ) {
      throw new Error(`kubernetesClusters.${index} has invalid field types`);
    }
    if (!/^[a-z0-9][a-z0-9-]{0,62}$/.test(cluster.name)) {
      throw new Error("kubernetesClusters names must be safe DNS labels");
    }
    if (names.has(cluster.name)) {
      throw new Error("kubernetesClusters names must be unique");
    }
    names.add(cluster.name);
    if (!/^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$/.test(cluster.context)) {
      throw new Error("kubernetesClusters contexts must be nonempty safe names");
    }
    if (contexts.has(cluster.context)) {
      throw new Error("kubernetesClusters contexts must be unique");
    }
    contexts.add(cluster.context);
    const server = validateHttpsOrigin(
      cluster.server,
      `kubernetesClusters.${cluster.name}.server`,
    );
    const parsedServer = new URL(server);
    const apiServerPort = Number(parsedServer.port || "443");
    if (!Number.isInteger(apiServerPort) || apiServerPort < 1 || apiServerPort > 65_535) {
      throw new Error("kubernetesClusters server ports must be between 1 and 65535");
    }
    if (
      cluster.apiServerEndpointCidrs.length === 0 ||
      cluster.apiServerEndpointCidrs.some((cidr) => {
        const [address, prefix, extra] = cidr.split("/");
        const family = isIP(address);
        const prefixNumber = Number(prefix);
        return (
          extra !== undefined ||
          family === 0 ||
          !/^(?:0|[1-9]\d*)$/.test(prefix ?? "") ||
          !Number.isInteger(prefixNumber) ||
          prefixNumber < 0 ||
          prefixNumber > (family === 4 ? 32 : 128)
        );
      })
    ) {
      throw new Error(
        "kubernetesClusters apiServerEndpointCidrs must contain valid CIDRs",
      );
    }
    return {
      name: cluster.name,
      context: cluster.context,
      server,
      apiServerEndpointCidrs: [...cluster.apiServerEndpointCidrs],
      apiServerPort,
    };
  });
}
