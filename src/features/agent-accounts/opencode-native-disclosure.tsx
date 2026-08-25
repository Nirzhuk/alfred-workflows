type OpenCodeNativeDisclosureProps = {
  connectAvailable: boolean;
};

export function OpenCodeNativeDisclosure({
  connectAvailable,
}: OpenCodeNativeDisclosureProps) {
  return (
    <div className="native-agent-provider-disclosure" role="note">
      <p className="settings-value">
        OpenCode 1.18.23 is a runtime/router. A native account must name the
        actual upstream provider, its auth method, and its billing owner; an
        OpenCode login never grants unrelated provider subscriptions.
      </p>
      {!connectAvailable ? (
        <p className="settings-value native-agent-gate">
          Native connect is unavailable until Alfred ships a verified signed
          runtime, a secure non-UI upstream credential entry flow, and an
          official typed bridge for Alfred-owned tool execution. The OpenCode
          CLI harness remains separate.
        </p>
      ) : null}
    </div>
  );
}
