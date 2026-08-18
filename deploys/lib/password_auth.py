from collections.abc import Callable, Iterator
from itertools import islice

from paramiko import Agent
from paramiko.auth_strategy import AuthResult, AuthSource, AuthStrategy, InMemoryPrivateKey, SourceResult
from paramiko.ssh_exception import AuthenticationException, BadAuthenticationType, SSHException


MAX_AGENT_KEY_ATTEMPTS = 4


class MemorySecret:
    def __init__(self, value: str) -> None:
        self.__value = value

    def reveal(self) -> str:
        return self.__value

    def __repr__(self) -> str:
        return "MemorySecret(<redacted>)"


class LazySecret:
    def __init__(self, getter: Callable[[], str], empty_error: str) -> None:
        self.__getter = getter
        self.__empty_error = empty_error
        self.__secret: MemorySecret | None = None

    @property
    def is_set(self) -> bool:
        return self.__secret is not None

    def reveal(self) -> str:
        if self.__secret is None:
            value = self.__getter()
            if not value:
                raise ValueError(self.__empty_error)
            self.__secret = MemorySecret(value)
        return self.__secret.reveal()

    def __repr__(self) -> str:
        return "LazySecret(<redacted>)"


class _MethodProbe(AuthSource):
    def authenticate(self, transport):
        return transport.auth_none(self.username)

    def __repr__(self) -> str:
        return f"MethodProbe(user={self.username!r})"


class _AgentKeySource(InMemoryPrivateKey):
    def __repr__(self) -> str:
        try:
            algorithm = self.pkey.get_name()
            fingerprint = self.pkey.fingerprint
            if not isinstance(algorithm, str) or not 0 < len(algorithm) <= 64:
                algorithm = "unknown"
            if not isinstance(fingerprint, str) or not 0 < len(fingerprint) <= 128:
                fingerprint = "unavailable"
        except Exception:
            algorithm = "unknown"
            fingerprint = "unavailable"
        return f"AgentKeySource(algorithm={algorithm!r}, fingerprint={fingerprint!r})"


class _StrictPasswordSource(AuthSource):
    def __init__(self, username: str, password_getter: Callable[[], str]) -> None:
        super().__init__(username=username)
        self.__password_getter = password_getter

    def authenticate(self, transport):
        return transport.auth_password(
            self.username,
            self.__password_getter(),
            fallback=False,
        )

    def __repr__(self) -> str:
        return f"StrictPasswordSource(user={self.username!r})"


class AgentFirstAuthStrategy(AuthStrategy):
    def __init__(
        self,
        username: str,
        password_getter: Callable[[], str],
        agent_factory: Callable[[], Agent] = Agent,
    ) -> None:
        super().__init__(ssh_config=None)
        self._username = username
        self._password_getter = password_getter
        self._agent_factory = agent_factory

    def get_sources(self) -> Iterator[AuthSource]:
        return iter(())

    def authenticate(self, transport) -> AuthResult:
        result = AuthResult(strategy=self)
        probe = _MethodProbe(username=self._username)
        try:
            allowed = probe.authenticate(transport)
        except BadAuthenticationType as error:
            allowed = error.allowed_types
            result.append(SourceResult(probe, error))
        except (AuthenticationException, SSHException, OSError) as error:
            raise AuthenticationException(
                "SSH server authentication methods could not be determined"
            ) from None
        else:
            result.append(SourceResult(probe, allowed))
            raise AuthenticationException("SSH server unexpectedly accepted none authentication")

        if (
            not isinstance(allowed, (list, tuple))
            or len(allowed) > 16
            or any(not isinstance(method, str) or not method or len(method) > 64 for method in allowed)
        ):
            raise AuthenticationException("SSH server returned an invalid authentication method list")
        allowed_methods = set(allowed)

        if "publickey" in allowed_methods:
            agent = None
            try:
                agent = self._agent_factory()
                for key in islice(agent.get_keys(), MAX_AGENT_KEY_ATTEMPTS):
                    source = _AgentKeySource(username=self._username, pkey=key)
                    try:
                        attempt = source.authenticate(transport)
                    except AuthenticationException as error:
                        result.append(SourceResult(source, error))
                        continue
                    except (SSHException, OSError):
                        raise AuthenticationException(
                            "SSH agent authentication could not be completed"
                        ) from None
                    result.append(SourceResult(source, attempt))
                    if attempt:
                        raise AuthenticationException(
                            "SSH server requires unsupported multi-step authentication"
                        )
                    return result
            except AuthenticationException:
                raise
            except (SSHException, OSError):
                raise AuthenticationException("SSH agent could not be queried") from None
            except Exception:
                raise AuthenticationException("SSH agent returned an invalid response") from None
            finally:
                if agent is not None:
                    try:
                        agent.close()
                    except Exception:
                        raise AuthenticationException("SSH agent could not be closed") from None

        if "password" in allowed_methods:
            source = _StrictPasswordSource(
                username=self._username,
                password_getter=self._password_getter,
            )
            try:
                attempt = source.authenticate(transport)
            except AuthenticationException:
                raise AuthenticationException("SSH password authentication failed") from None
            except (SSHException, OSError):
                raise AuthenticationException(
                    "SSH password authentication could not be completed"
                ) from None
            result.append(SourceResult(source, attempt))
            if attempt:
                raise AuthenticationException("SSH server requires unsupported multi-step authentication")
            return result

        if "publickey" in allowed_methods:
            raise AuthenticationException(
                "SSH server permits publickey authentication, but no bounded agent identity succeeded"
            )
        raise AuthenticationException("SSH server offers no supported authentication method")

    def __repr__(self) -> str:
        return f"AgentFirstAuthStrategy(username={self._username!r})"
