type CursorNativeDisclosureProps = {
  connectAvailable: boolean;
};

export function CursorNativeDisclosure({
  connectAvailable,
}: CursorNativeDisclosureProps) {
  return (
    <p className="settings-value native-agent-gate">
      Cursor native uses a Cursor API key billed through Cursor Cloud. Work is
      executed remotely against an explicitly confirmed remote repository and
      starting ref; it does not reuse local Cursor CLI credentials or a
      runtime-managed local sign-in.
      {!connectAvailable
        ? " Native setup remains disabled until secure API-key intake, explicit repository consent, and an Alfred-compatible approval boundary ship."
        : null}
    </p>
  );
}
