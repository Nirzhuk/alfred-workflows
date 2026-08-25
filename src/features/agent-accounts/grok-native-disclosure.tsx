type GrokNativeDisclosureProps = {
  connectAvailable: boolean;
};

export function GrokNativeDisclosure({
  connectAvailable,
}: GrokNativeDisclosureProps) {
  return (
    <p className="settings-value native-agent-gate">
      xAI API usage is billed to your xAI API team. A Grok subscription and
      Grok Build sign-in do not apply.
      {!connectAvailable
        ? " Native setup remains disabled until Alfred has an approved secure API-key entry flow."
        : null}
    </p>
  );
}
