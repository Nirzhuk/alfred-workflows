from __future__ import annotations

import argparse
import json
import os
import re
import sys
import threading
from pathlib import Path
from typing import Any, NoReturn

from openai_codex import ApprovalMode, Codex, CodexConfig, Sandbox, __version__

from . import SIDECAR_PROTOCOL_VERSION

SDK_VERSION = "0.147.0"
MAX_FRAME_BYTES = 256 * 1024
MAX_ID_BYTES = 128
MAX_MODEL_BYTES = 256
MAX_PROMPT_BYTES = 128 * 1024
MAX_DELTA_BYTES = 64 * 1024
MAX_THREADS = 64
HOST_APPROVAL_BLOCKER = "codex_python_sdk_host_approval_unavailable"

READY_FRAME = {
    "experimentalApi": False,
    "protocolVersion": SIDECAR_PROTOCOL_VERSION,
    "sdkVersion": SDK_VERSION,
    "type": "ready",
}

REQUEST_ID = re.compile(r"^[A-Za-z0-9_-]{1,128}$")
WIRE_ID = re.compile(r"^[A-Za-z0-9._:-]{1,256}$")

PROHIBITED_AMBIENT_AUTH = (
    "OPENAI_API_KEY",
    "OPENAI_ACCESS_TOKEN",
    "CODEX_ACCESS_TOKEN",
    "CODEX_API_KEY",
    "OPENAI_BASE_URL",
    "OPENAI_API_BASE",
    "OPENAI_ORG_ID",
    "OPENAI_PROJECT_ID",
)

CODEX_CONFIG_OVERRIDES = (
    'cli_auth_credentials_store="keyring"',
    'mcp_oauth_credentials_store="keyring"',
    'forced_login_method="chatgpt"',
    "check_for_update_on_startup=false",
    "tools.web_search=false",
)


class ProtocolFailure(Exception):
    def __init__(self, code: str, *, retryable: bool = False) -> None:
        super().__init__(code)
        self.code = code
        self.retryable = retryable


def _reject_json_constant(_value: str) -> NoReturn:
    raise ProtocolFailure("codex_sidecar_malformed_frame")


def _strict_json_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ProtocolFailure("codex_sidecar_malformed_frame")
        result[key] = value
    return result


def _strict_json_loads(body: bytes) -> Any:
    return json.loads(
        body,
        object_pairs_hook=_strict_json_object,
        parse_constant=_reject_json_constant,
    )


def _json_bytes(value: dict[str, Any]) -> bytes:
    encoded = json.dumps(
        value,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")
    if not encoded or len(encoded) > MAX_FRAME_BYTES or b"\n" in encoded:
        raise ProtocolFailure("codex_sidecar_output_limit_exceeded")
    return encoded


class Writer:
    def __init__(self) -> None:
        self._lock = threading.Lock()

    def send(self, value: dict[str, Any]) -> None:
        encoded = _json_bytes(value)
        with self._lock:
            sys.stdout.buffer.write(encoded)
            sys.stdout.buffer.write(b"\n")
            sys.stdout.buffer.flush()

    def response(self, request_id: str, method: str, result: dict[str, Any]) -> None:
        self.send(
            {
                "method": method,
                "protocolVersion": SIDECAR_PROTOCOL_VERSION,
                "requestId": request_id,
                "result": result,
                "type": "response",
            }
        )

    def error(self, request_id: str | None, code: str, *, retryable: bool = False) -> None:
        self.send(
            {
                "code": code,
                "protocolVersion": SIDECAR_PROTOCOL_VERSION,
                "requestId": request_id,
                "retryable": retryable,
                "type": "error",
            }
        )

    def event(self, operation_id: str, event: dict[str, Any]) -> None:
        self.send(
            {
                "event": event,
                "operationId": operation_id,
                "protocolVersion": SIDECAR_PROTOCOL_VERSION,
                "type": "event",
            }
        )


def _require_object(value: Any, code: str = "codex_sidecar_invalid_request") -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ProtocolFailure(code)
    return value


def _require_keys(
    value: dict[str, Any],
    *,
    required: set[str] = frozenset(),
    optional: set[str] = frozenset(),
) -> None:
    keys = set(value)
    if not required.issubset(keys) or not keys.issubset(required | optional):
        raise ProtocolFailure("codex_sidecar_invalid_request")


def _bounded_string(value: Any, maximum: int, *, pattern: re.Pattern[str] | None = None) -> str:
    if not isinstance(value, str):
        raise ProtocolFailure("codex_sidecar_invalid_request")
    encoded = value.encode("utf-8")
    if not encoded or len(encoded) > maximum or "\x00" in value:
        raise ProtocolFailure("codex_sidecar_invalid_request")
    if pattern is not None and pattern.fullmatch(value) is None:
        raise ProtocolFailure("codex_sidecar_invalid_request")
    return value


def _absolute_directory(value: Any) -> str:
    raw = _bounded_string(value, 4096)
    path = Path(raw)
    if not path.is_absolute() or not path.is_dir() or path.is_symlink():
        raise ProtocolFailure("codex_sidecar_invalid_cwd")
    return str(path.resolve(strict=True))


def _dump_model(value: Any) -> dict[str, Any]:
    model_dump = getattr(value, "model_dump", None)
    if not callable(model_dump):
        raise ProtocolFailure("codex_sidecar_sdk_contract_mismatch")
    dumped = model_dump(mode="json", by_alias=True, exclude_none=True)
    if not isinstance(dumped, dict):
        raise ProtocolFailure("codex_sidecar_sdk_contract_mismatch")
    return dumped


def _enum_value(value: Any) -> str:
    raw = getattr(value, "value", value)
    if not isinstance(raw, str):
        raise ProtocolFailure("codex_sidecar_sdk_contract_mismatch")
    return raw


def _resolve_codex_binary(explicit: str | None) -> Path:
    if explicit is not None:
        candidate = Path(explicit)
        if not candidate.is_absolute():
            raise ProtocolFailure("codex_sidecar_packaged_runtime_invalid")
    else:
        launcher = Path(sys.executable if getattr(sys, "frozen", False) else sys.argv[0])
        package_root = launcher.resolve(strict=True).parent.parent
        executable = "codex.exe" if os.name == "nt" else "codex"
        candidate = package_root / "libexec" / executable
    if candidate.is_symlink():
        raise ProtocolFailure("codex_sidecar_packaged_runtime_invalid")
    resolved = candidate.resolve(strict=True)
    if not resolved.is_file():
        raise ProtocolFailure("codex_sidecar_packaged_runtime_invalid")
    return resolved


def _validate_profile_environment() -> Path:
    for key in PROHIBITED_AMBIENT_AUTH:
        if os.environ.get(key):
            raise ProtocolFailure("codex_sidecar_ambient_auth_rejected")
    raw = os.environ.get("CODEX_HOME")
    if not raw:
        raise ProtocolFailure("codex_sidecar_profile_missing")
    home = Path(raw)
    if not home.is_absolute() or not home.is_dir() or home.is_symlink():
        raise ProtocolFailure("codex_sidecar_profile_invalid")
    resolved = home.resolve(strict=True)
    if (resolved / "auth.json").exists():
        raise ProtocolFailure("codex_sidecar_auth_file_import_rejected")
    return resolved


class Sidecar:
    def __init__(self, codex_bin: Path, writer: Writer) -> None:
        if __version__ != SDK_VERSION:
            raise ProtocolFailure("codex_sidecar_sdk_version_mismatch")
        self._profile_home = _validate_profile_environment()
        cwd = _absolute_directory(os.getcwd())
        self._cwd = cwd
        self._writer = writer
        self._lock = threading.Lock()
        self._threads: dict[str, Any] = {}
        self._turns: dict[str, Any] = {}
        self._logins: dict[str, Any] = {}
        self._login_waiters: set[str] = set()
        self._deferred_workers: dict[str, threading.Thread] = {}
        self._closed = False
        self._codex = Codex(
            CodexConfig(
                codex_bin=str(codex_bin),
                config_overrides=CODEX_CONFIG_OVERRIDES,
                cwd=cwd,
                client_name="alfred_desktop",
                client_title="Alfred Desktop",
                client_version="codex-sdk-sidecar/0.1.0",
                experimental_api=False,
            )
        )

    def close(self) -> None:
        with self._lock:
            if self._closed:
                return
            self._closed = True
        try:
            self._codex.close()
        except Exception as error:
            raise ProtocolFailure("codex_sidecar_sdk_failure", retryable=True) from error

    def dispatch(self, request: dict[str, Any]) -> bool:
        _require_keys(
            request,
            required={"protocolVersion", "requestId", "method", "params"},
        )
        if (
            type(request["protocolVersion"]) is not int
            or request["protocolVersion"] != SIDECAR_PROTOCOL_VERSION
        ):
            raise ProtocolFailure("codex_sidecar_protocol_mismatch")
        request_id = _bounded_string(request["requestId"], MAX_ID_BYTES, pattern=REQUEST_ID)
        method = _bounded_string(request["method"], 64)
        params = _require_object(request["params"])

        handlers = {
            "capabilities": self._capabilities,
            "login_start": self._login_start,
            "login_wait": self._login_wait,
            "login_cancel": self._login_cancel,
            "account": self._account,
            "logout": self._logout,
            "models": self._models,
            "thread_start": self._thread_start,
            "thread_resume": self._thread_resume,
            "turn_start": self._turn_start,
            "turn_cancel": self._turn_cancel,
            "approval_decide": self._approval_decide,
            "shutdown": self._shutdown,
        }
        handler = handlers.get(method)
        if handler is None:
            raise ProtocolFailure("codex_sidecar_method_unsupported")
        result = handler(request_id, params)
        if result is not None:
            self._writer.response(request_id, method, result)
        with self._lock:
            worker = self._deferred_workers.pop(request_id, None)
        if worker is not None:
            worker.start()
        return method != "shutdown"

    def _capabilities(self, _request_id: str, params: dict[str, Any]) -> dict[str, Any]:
        _require_keys(params)
        return {
            "account": True,
            "browserLogin": True,
            "deviceCodeLogin": True,
            "experimentalApi": False,
            "hostApprovalBlocker": HOST_APPROVAL_BLOCKER,
            "hostApprovals": False,
            "logout": True,
            "models": True,
            "sdkVersion": SDK_VERSION,
            "streamedTurns": True,
            "threadCreate": True,
            "threadResume": True,
            "turnCancellation": True,
            "usage": False,
        }

    def _login_start(self, _request_id: str, params: dict[str, Any]) -> dict[str, Any]:
        _require_keys(params, required={"kind"})
        kind = _bounded_string(params["kind"], 32)
        with self._lock:
            if self._logins:
                raise ProtocolFailure("codex_sidecar_login_busy")
        if kind == "browser":
            handle = self._codex.login_chatgpt()
            result = {
                "authorizationUrl": _bounded_string(handle.auth_url, 4096),
                "kind": kind,
                "loginId": _bounded_string(handle.login_id, MAX_ID_BYTES, pattern=WIRE_ID),
                "userCode": None,
            }
        elif kind == "device_code":
            handle = self._codex.login_chatgpt_device_code()
            result = {
                "authorizationUrl": _bounded_string(handle.verification_url, 4096),
                "kind": kind,
                "loginId": _bounded_string(handle.login_id, MAX_ID_BYTES, pattern=WIRE_ID),
                "userCode": _bounded_string(handle.user_code, 64),
            }
        else:
            raise ProtocolFailure("codex_sidecar_invalid_request")
        with self._lock:
            self._logins[result["loginId"]] = handle
        return result

    def _login_wait(self, request_id: str, params: dict[str, Any]) -> dict[str, Any]:
        _require_keys(params, required={"loginId"})
        login_id = _bounded_string(params["loginId"], MAX_ID_BYTES, pattern=WIRE_ID)
        with self._lock:
            handle = self._logins.get(login_id)
            if handle is None:
                raise ProtocolFailure("codex_sidecar_login_unavailable")
            if login_id in self._login_waiters:
                raise ProtocolFailure("codex_sidecar_login_busy")
            self._login_waiters.add(login_id)
            self._deferred_workers[request_id] = threading.Thread(
                target=self._wait_for_login,
                args=(login_id, handle),
                daemon=True,
            )
        return {"accepted": True, "loginId": login_id}

    def _wait_for_login(self, login_id: str, handle: Any) -> None:
        success = False
        try:
            completion = handle.wait()
            self._assert_keyring_custody()
            success = bool(getattr(completion, "success", False))
        except Exception:
            success = False
        finally:
            with self._lock:
                self._logins.pop(login_id, None)
                self._login_waiters.discard(login_id)
        self._writer.event(
            login_id,
            {"kind": "login_completed", "loginId": login_id, "success": success},
        )

    def _login_cancel(self, _request_id: str, params: dict[str, Any]) -> dict[str, Any]:
        _require_keys(params, required={"loginId"})
        login_id = _bounded_string(params["loginId"], MAX_ID_BYTES, pattern=WIRE_ID)
        with self._lock:
            handle = self._logins.get(login_id)
        if handle is None:
            raise ProtocolFailure("codex_sidecar_login_unavailable")
        handle.cancel()
        return {"cancelled": True, "loginId": login_id}

    def _account_payload(self) -> dict[str, Any]:
        self._assert_keyring_custody()
        dumped = _dump_model(self._codex.account(refresh_token=False))
        self._assert_keyring_custody()
        _require_keys(dumped, required={"requiresOpenaiAuth"}, optional={"account"})
        account = dumped.get("account")
        if account is None:
            return {"authenticated": False}
        account = _require_object(account, "codex_sidecar_sdk_contract_mismatch")
        if account.get("type") != "chatgpt":
            raise ProtocolFailure("codex_sidecar_account_product_mismatch")
        email = account.get("email")
        plan_type = account.get("planType")
        if email is not None:
            email = _bounded_string(email, 320)
        if plan_type is not None:
            plan_type = _bounded_string(plan_type, 128)
        return {
            "authMode": "chatgpt",
            "authenticated": True,
            "displayLabel": email,
            "planType": plan_type,
            "requiresOpenaiAuth": bool(dumped["requiresOpenaiAuth"]),
        }

    def _account(self, _request_id: str, params: dict[str, Any]) -> dict[str, Any]:
        _require_keys(params)
        return self._account_payload()

    def _assert_keyring_custody(self) -> None:
        if (self._profile_home / "auth.json").exists():
            raise ProtocolFailure("codex_sidecar_auth_file_created")

    def _request_cwd(self, value: Any) -> str:
        cwd = _absolute_directory(value)
        if cwd != self._cwd:
            raise ProtocolFailure("codex_sidecar_invalid_cwd")
        return cwd

    def _logout(self, _request_id: str, params: dict[str, Any]) -> dict[str, Any]:
        _require_keys(params)
        with self._lock:
            if self._turns:
                raise ProtocolFailure("codex_sidecar_turn_busy")
            if self._logins:
                raise ProtocolFailure("codex_sidecar_login_busy")
        self._codex.logout()
        self._assert_keyring_custody()
        with self._lock:
            self._threads.clear()
        return {"loggedOut": True, "profileState": "logged_out"}

    def _models(self, _request_id: str, params: dict[str, Any]) -> dict[str, Any]:
        _require_keys(params)
        if not self._account_payload().get("authenticated"):
            raise ProtocolFailure("codex_sidecar_account_unavailable")
        dumped = _dump_model(self._codex.models(include_hidden=False))
        _require_keys(dumped, required={"data"}, optional={"nextCursor"})
        data = dumped["data"]
        if not isinstance(data, list) or len(data) > 256:
            raise ProtocolFailure("codex_sidecar_sdk_contract_mismatch")
        models = []
        for raw in data:
            model = _require_object(raw, "codex_sidecar_sdk_contract_mismatch")
            model_id = _bounded_string(model.get("id"), MAX_MODEL_BYTES)
            label = _bounded_string(model.get("displayName", model_id), MAX_MODEL_BYTES)
            models.append(
                {"id": model_id, "isDefault": bool(model.get("isDefault", False)), "label": label}
            )
        return {"models": models}

    def _thread_start(self, _request_id: str, params: dict[str, Any]) -> dict[str, Any]:
        _require_keys(params, required={"cwd", "model"})
        if not self._account_payload().get("authenticated"):
            raise ProtocolFailure("codex_sidecar_account_unavailable")
        cwd = self._request_cwd(params["cwd"])
        model = _bounded_string(params["model"], MAX_MODEL_BYTES)
        with self._lock:
            if len(self._threads) >= MAX_THREADS:
                raise ProtocolFailure("codex_sidecar_thread_limit_exceeded")
        thread = self._codex.thread_start(
            approval_mode=ApprovalMode.deny_all,
            cwd=cwd,
            ephemeral=False,
            model=model,
            sandbox=Sandbox.read_only,
        )
        thread_id = _bounded_string(thread.id, 256, pattern=WIRE_ID)
        with self._lock:
            self._threads[thread_id] = thread
        return {"threadId": thread_id}

    def _thread_resume(self, _request_id: str, params: dict[str, Any]) -> dict[str, Any]:
        _require_keys(params, required={"cwd", "model", "threadId"})
        if not self._account_payload().get("authenticated"):
            raise ProtocolFailure("codex_sidecar_account_unavailable")
        cwd = self._request_cwd(params["cwd"])
        model = _bounded_string(params["model"], MAX_MODEL_BYTES)
        thread_id = _bounded_string(params["threadId"], 256, pattern=WIRE_ID)
        thread = self._codex.thread_resume(
            thread_id,
            approval_mode=ApprovalMode.deny_all,
            cwd=cwd,
            model=model,
            sandbox=Sandbox.read_only,
        )
        resumed_id = _bounded_string(thread.id, 256, pattern=WIRE_ID)
        if resumed_id != thread_id:
            raise ProtocolFailure("codex_sidecar_session_mismatch")
        with self._lock:
            if len(self._threads) >= MAX_THREADS and thread_id not in self._threads:
                raise ProtocolFailure("codex_sidecar_thread_limit_exceeded")
            self._threads[thread_id] = thread
        return {"threadId": thread_id}

    def _turn_start(self, request_id: str, params: dict[str, Any]) -> dict[str, Any]:
        _require_keys(
            params,
            required={"cwd", "model", "operationId", "prompt", "threadId"},
        )
        cwd = self._request_cwd(params["cwd"])
        model = _bounded_string(params["model"], MAX_MODEL_BYTES)
        operation_id = _bounded_string(params["operationId"], MAX_ID_BYTES, pattern=REQUEST_ID)
        prompt = _bounded_string(params["prompt"], MAX_PROMPT_BYTES)
        thread_id = _bounded_string(params["threadId"], 256, pattern=WIRE_ID)
        with self._lock:
            thread = self._threads.get(thread_id)
            if thread is None:
                raise ProtocolFailure("codex_sidecar_session_unavailable")
            if self._turns:
                raise ProtocolFailure("codex_sidecar_turn_busy")
        turn = thread.turn(
            prompt,
            approval_mode=ApprovalMode.deny_all,
            cwd=cwd,
            model=model,
            sandbox=Sandbox.read_only,
        )
        turn_id = _bounded_string(turn.id, 256, pattern=WIRE_ID)
        with self._lock:
            self._turns[operation_id] = turn
            self._deferred_workers[request_id] = threading.Thread(
                target=self._stream_turn,
                args=(operation_id, thread_id, turn_id, turn),
                daemon=True,
            )
        return {"operationId": operation_id, "threadId": thread_id, "turnId": turn_id}

    def _stream_turn(self, operation_id: str, thread_id: str, turn_id: str, turn: Any) -> None:
        completed = False
        try:
            for notification in turn.stream():
                method = getattr(notification, "method", None)
                payload = getattr(notification, "payload", None)
                if method == "turn/started":
                    self._writer.event(
                        operation_id,
                        {"kind": "turn_started", "threadId": thread_id, "turnId": turn_id},
                    )
                elif method == "item/agentMessage/delta":
                    delta = _bounded_string(getattr(payload, "delta", None), MAX_DELTA_BYTES)
                    event_thread = _bounded_string(
                        getattr(payload, "thread_id", None), 256, pattern=WIRE_ID
                    )
                    event_turn = _bounded_string(
                        getattr(payload, "turn_id", None), 256, pattern=WIRE_ID
                    )
                    if event_thread != thread_id or event_turn != turn_id:
                        raise ProtocolFailure("codex_sidecar_event_mismatch")
                    self._writer.event(
                        operation_id,
                        {
                            "kind": "assistant_delta",
                            "text": delta,
                            "threadId": thread_id,
                            "turnId": turn_id,
                        },
                    )
                elif method == "turn/completed":
                    completed_turn = getattr(payload, "turn", None)
                    completed_id = _bounded_string(
                        getattr(completed_turn, "id", None), 256, pattern=WIRE_ID
                    )
                    if completed_id != turn_id:
                        raise ProtocolFailure("codex_sidecar_event_mismatch")
                    status = _enum_value(getattr(completed_turn, "status", None))
                    if status not in {"completed", "failed", "interrupted"}:
                        raise ProtocolFailure("codex_sidecar_sdk_contract_mismatch")
                    self._writer.event(
                        operation_id,
                        {
                            "kind": "turn_completed",
                            "status": status,
                            "threadId": thread_id,
                            "turnId": turn_id,
                        },
                    )
                    completed = True
                else:
                    # Reasoning, raw response, command/file output, tool state,
                    # usage, warnings, and unknown notifications never cross
                    # the sidecar boundary.
                    continue
            if not completed:
                raise ProtocolFailure("codex_sidecar_stream_incomplete", retryable=True)
        except ProtocolFailure as error:
            self._writer.error(operation_id, error.code, retryable=error.retryable)
        except Exception:
            self._writer.error(operation_id, "codex_sidecar_sdk_failure", retryable=True)
        finally:
            with self._lock:
                self._turns.pop(operation_id, None)

    def _turn_cancel(self, _request_id: str, params: dict[str, Any]) -> dict[str, Any]:
        _require_keys(params, required={"operationId"})
        operation_id = _bounded_string(params["operationId"], MAX_ID_BYTES, pattern=REQUEST_ID)
        with self._lock:
            turn = self._turns.get(operation_id)
        if turn is None:
            raise ProtocolFailure("codex_sidecar_turn_unavailable")
        turn.interrupt()
        return {"cancelled": True, "operationId": operation_id}

    def _approval_decide(self, _request_id: str, params: dict[str, Any]) -> dict[str, Any]:
        _require_keys(params, required={"approvalId", "decision"})
        _bounded_string(params["approvalId"], MAX_ID_BYTES, pattern=REQUEST_ID)
        decision = _bounded_string(params["decision"], 16)
        if decision not in {"allow", "deny"}:
            raise ProtocolFailure("codex_sidecar_invalid_request")
        raise ProtocolFailure(HOST_APPROVAL_BLOCKER)

    def _shutdown(self, _request_id: str, params: dict[str, Any]) -> dict[str, Any]:
        _require_keys(params)
        self.close()
        return {"closed": True}


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--codex-bin")
    return parser.parse_args()


def main() -> int:
    writer = Writer()
    sidecar: Sidecar | None = None
    try:
        args = _parse_args()
        sidecar = Sidecar(_resolve_codex_binary(args.codex_bin), writer)
        writer.send(READY_FRAME)
        while True:
            line = sys.stdin.buffer.readline(MAX_FRAME_BYTES + 2)
            if not line:
                break
            if len(line) > MAX_FRAME_BYTES + 1 or not line.endswith(b"\n"):
                raise ProtocolFailure("codex_sidecar_frame_too_large")
            body = line[:-1]
            if body.endswith(b"\r"):
                body = body[:-1]
            if not body:
                raise ProtocolFailure("codex_sidecar_malformed_frame")
            request_id: str | None = None
            try:
                parsed = _strict_json_loads(body)
                request = _require_object(parsed, "codex_sidecar_malformed_frame")
                candidate_id = request.get("requestId")
                if isinstance(candidate_id, str) and REQUEST_ID.fullmatch(candidate_id):
                    request_id = candidate_id
                if not sidecar.dispatch(request):
                    break
            except (UnicodeDecodeError, json.JSONDecodeError):
                writer.error(request_id, "codex_sidecar_malformed_frame")
            except ProtocolFailure as error:
                writer.error(request_id, error.code, retryable=error.retryable)
            except Exception:
                writer.error(request_id, "codex_sidecar_sdk_failure", retryable=True)
        return 0
    except ProtocolFailure as error:
        sys.stderr.write(error.code + "\n")
        return 2
    except Exception:
        sys.stderr.write("codex_sidecar_startup_failed\n")
        return 2
    finally:
        if sidecar is not None:
            try:
                sidecar.close()
            except Exception:
                pass


if __name__ == "__main__":
    raise SystemExit(main())
