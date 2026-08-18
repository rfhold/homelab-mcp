import json
import os
import stat
import tempfile
from dataclasses import dataclass
from pathlib import Path


MAX_CACHE_BYTES = 65_536
MAX_TOKEN_BYTES = 16_384
APP_DIRECTORY = "homelab-mcp"
CACHE_FILENAME = "ssh-token.json"


@dataclass(frozen=True)
class CachedToken:
    origin: str
    token: str


class TokenCache:
    def __init__(self, runtime_root: Path, uid: int | None = None):
        if not runtime_root.is_absolute():
            raise ValueError("XDG_RUNTIME_DIR must be an absolute path")
        self.uid = os.getuid() if uid is None else uid
        self.runtime_root = runtime_root
        self.directory = runtime_root / APP_DIRECTORY
        self.path = self.directory / CACHE_FILENAME
        self._validate_runtime_root()

    @classmethod
    def from_environment(cls) -> "TokenCache":
        value = os.environ.get("XDG_RUNTIME_DIR")
        if not value:
            raise ValueError("XDG_RUNTIME_DIR is required for the OpenBao token cache")
        root = Path(value)
        if not root.is_absolute():
            raise ValueError("XDG_RUNTIME_DIR must be an absolute path")
        return cls(root)

    def _validate_runtime_root(self) -> None:
        try:
            metadata = os.lstat(self.runtime_root)
        except FileNotFoundError as error:
            raise ValueError("XDG_RUNTIME_DIR must name an existing secure directory") from error
        if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISDIR(metadata.st_mode):
            raise ValueError("XDG_RUNTIME_DIR must be a real directory, not a symlink")
        if metadata.st_uid != self.uid:
            raise ValueError("XDG_RUNTIME_DIR must be owned by the current user")
        if stat.S_IMODE(metadata.st_mode) & 0o077:
            raise ValueError("XDG_RUNTIME_DIR must not grant group or other permissions")

    def _validate_directory(self) -> bool:
        try:
            metadata = os.lstat(self.directory)
        except FileNotFoundError:
            return False
        if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISDIR(metadata.st_mode):
            raise ValueError("OpenBao token cache directory must be a real directory")
        if metadata.st_uid != self.uid:
            raise ValueError("OpenBao token cache directory has the wrong owner")
        if stat.S_IMODE(metadata.st_mode) != 0o700:
            raise ValueError("OpenBao token cache directory must have mode 0700")
        return True

    def _ensure_directory(self) -> None:
        if not self._validate_directory():
            try:
                self.directory.mkdir(mode=0o700)
            except FileExistsError:
                pass
            if not self._validate_directory():
                raise ValueError("OpenBao token cache directory could not be created securely")

    def _open_cache(self) -> int | None:
        if not self._validate_directory():
            return None
        flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
        try:
            descriptor = os.open(self.path, flags)
        except FileNotFoundError:
            return None
        except OSError as error:
            raise ValueError("OpenBao token cache must be a regular non-symlink file") from error
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != self.uid
            or stat.S_IMODE(metadata.st_mode) != 0o600
        ):
            os.close(descriptor)
            raise ValueError("OpenBao token cache must be an owner-owned mode 0600 regular file")
        return descriptor

    def load(self, expected_origin: str) -> CachedToken | None:
        descriptor = self._open_cache()
        if descriptor is None:
            return None
        try:
            with os.fdopen(descriptor, "rb") as cache_file:
                content = cache_file.read(MAX_CACHE_BYTES + 1)
        except OSError as error:
            raise ValueError("OpenBao token cache could not be read safely") from error
        try:
            if len(content) > MAX_CACHE_BYTES:
                raise ValueError
            value = json.loads(content)
            if not isinstance(value, dict) or set(value) != {"schema", "origin", "token"}:
                raise ValueError
            origin = value["origin"]
            token = value["token"]
            if type(value["schema"]) is not int or value["schema"] != 1:
                raise ValueError
            if not isinstance(origin, str) or not isinstance(token, str):
                raise ValueError
            if not token or len(token.encode("utf-8")) > MAX_TOKEN_BYTES or any(c.isspace() for c in token):
                raise ValueError
        except (UnicodeDecodeError, json.JSONDecodeError, TypeError, ValueError):
            self.clear()
            return None
        if origin != expected_origin:
            self.clear()
            return None
        return CachedToken(origin=origin, token=token)

    def save(self, origin: str, token: str) -> None:
        if not token or len(token.encode("utf-8")) > MAX_TOKEN_BYTES or any(c.isspace() for c in token):
            raise ValueError("OpenBao token is invalid for caching")
        content = json.dumps(
            {"schema": 1, "origin": origin, "token": token},
            ensure_ascii=True,
            separators=(",", ":"),
        ).encode("utf-8")
        if len(content) > MAX_CACHE_BYTES:
            raise ValueError("OpenBao token cache content exceeds its bound")
        self._ensure_directory()
        existing = self._open_cache()
        if existing is not None:
            os.close(existing)
        descriptor, temporary_name = tempfile.mkstemp(prefix=".ssh-token-", dir=self.directory)
        temporary = Path(temporary_name)
        try:
            os.fchmod(descriptor, 0o600)
            with os.fdopen(descriptor, "wb") as cache_file:
                cache_file.write(content)
                cache_file.flush()
                os.fsync(cache_file.fileno())
            os.replace(temporary, self.path)
            directory_descriptor = os.open(
                self.directory,
                os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_CLOEXEC", 0),
            )
            try:
                os.fsync(directory_descriptor)
            finally:
                os.close(directory_descriptor)
        finally:
            try:
                os.close(descriptor)
            except OSError:
                pass
            try:
                temporary.unlink()
            except FileNotFoundError:
                pass

    def clear(self) -> None:
        descriptor = self._open_cache()
        if descriptor is None:
            return
        os.close(descriptor)
        try:
            self.path.unlink()
        except FileNotFoundError:
            pass
