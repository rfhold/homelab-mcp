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
