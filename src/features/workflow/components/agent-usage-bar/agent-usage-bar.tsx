import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { AgentMark, agentLabel } from "../../../../components/agent-mark";
import type {
  AgentProviderId,
  AgentUsageSnapshot,
  AgentUsageWindow,
} from "../../types";

type Props = {
  workflowKey: string;
  providers: AgentProviderId[];
  usage: AgentUsageSnapshot[];
  refreshing: boolean;
  onRefresh: () => void;
};

function clampPercent(value: number) {
  return Math.min(100, Math.max(0, Math.round(value)));
}

function formatCountdown(nowSeconds: number, resetsAt?: number | null) {
  if (!resetsAt) return null;
  const remainingSeconds = Math.max(0, resetsAt - nowSeconds);
  const days = Math.floor(remainingSeconds / 86_400);
  const hours = Math.floor((remainingSeconds % 86_400) / 3_600);
  const minutes = Math.floor((remainingSeconds % 3_600) / 60);
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  return `${minutes}m`;
}

function resetText(window: AgentUsageWindow, nowSeconds: number) {
  const countdown = formatCountdown(nowSeconds, window.resetsAt);
  return countdown ? `resets in ${countdown}` : window.resetDescription;
}

function windowText(window: AgentUsageWindow, nowSeconds: number) {
  const used = clampPercent(window.usedPercent);
  const reset = resetText(window, nowSeconds);
  return `${used}% used${reset ? `, ${reset}` : ""}`;
}

export function primaryUsageWindow(windows: AgentUsageWindow[]) {
  return (
    windows.find((window) => {
      const label = window.label.toLowerCase();
      return label.includes("5-hour") || label.includes("5 hour");
    }) ?? windows[0]
  );
}

function usageAgentLabel(
  provider: AgentProviderId,
  snapshot?: AgentUsageSnapshot,
) {
  return provider === "opencode" &&
    `${snapshot?.source ?? ""} ${snapshot?.error ?? ""}`
      .toLowerCase()
      .includes("opencode go")
    ? "OpenCode Go"
    : agentLabel(provider);
}

function usageTitle(
  provider: AgentProviderId,
  nowSeconds: number,
  snapshot?: AgentUsageSnapshot,
) {
  const label = usageAgentLabel(provider, snapshot);
  if (!snapshot) return `${label}: checking connection`;
  if (!snapshot.connected) return `${label}: ${snapshot.error ?? "Not connected"}`;
  if (snapshot.windows.length === 0) {
    return `${label}: ${snapshot.error ?? "Subscription usage unavailable"}`;
  }
  return `${label}: ${snapshot.windows
    .map((window) => `${window.label} ${windowText(window, nowSeconds)}`)
    .join(" · ")} · Source: ${snapshot.source}`;
}

function RefreshGlyph() {
  return (
    <svg width="12" height="12" viewBox="0 0 16 16" fill="none" aria-hidden>
      <path
        d="M12.7 5.35A5.25 5.25 0 1 0 13.1 9M12.7 2.8v2.9H9.8"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

type UsageWindowProps = {
  agent: string;
  estimate: boolean;
  nowSeconds: number;
  window: AgentUsageWindow;
};

function UsageWindow({
  agent,
  estimate,
  nowSeconds,
  window,
}: UsageWindowProps) {
  const remaining = 100 - clampPercent(window.usedPercent);
  const reset = resetText(window, nowSeconds);

  return (
    <div className="agent-usage-window" title={windowText(window, nowSeconds)}>
      <div className="agent-usage-window-copy">
        <span className="agent-usage-window-label">{window.label}</span>
        <span className="agent-usage-percent">
          {estimate ? "≈ " : ""}
          {remaining}% left
        </span>
      </div>
      <span
        className="agent-usage-track"
        role="progressbar"
        aria-label={`${agent} ${window.label} subscription remaining`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={remaining}
      >
        <span
          className="agent-usage-fill"
          style={{
            "--usage-remaining": remaining / 100,
          } as CSSProperties}
        />
      </span>
      <span
        className={`agent-usage-window-reset${reset ? "" : " is-placeholder"}`}
        aria-hidden={reset ? undefined : true}
      >
        {reset || "\u00a0"}
      </span>
    </div>
  );
}

type UsagePopoverProps = {
  agent: string;
  estimate: boolean;
  id: string;
  nowSeconds: number;
  onClose: () => void;
  open: boolean;
  provider: AgentProviderId;
  source: string;
  triggerRef: RefObject<HTMLButtonElement | null>;
  windows: AgentUsageWindow[];
};

function UsagePopover({
  agent,
  estimate,
  id,
  nowSeconds,
  onClose,
  open,
  provider,
  source,
  triggerRef,
  windows,
}: UsagePopoverProps) {
  const popoverRef = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<{ left: number; top: number } | null>(
    null,
  );

  const updatePosition = useCallback(() => {
    const trigger = triggerRef.current;
    const popover = popoverRef.current;
    if (!trigger || !popover) return;

    const triggerRect = trigger.getBoundingClientRect();
    const width = popover.offsetWidth;
    const height = popover.offsetHeight;
    const edge = 8;
    const gap = 8;
    const left = Math.min(
      Math.max(edge, triggerRect.left),
      window.innerWidth - width - edge,
    );
    const top = Math.max(edge, triggerRect.top - height - gap);
    setPosition({ left, top });
  }, [triggerRef]);

  useLayoutEffect(() => {
    if (!open) {
      setPosition(null);
      return;
    }

    updatePosition();
    const frame = window.requestAnimationFrame(updatePosition);
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);
    return () => {
      window.cancelAnimationFrame(frame);
      window.removeEventListener("resize", updatePosition);
      window.removeEventListener("scroll", updatePosition, true);
    };
  }, [open, updatePosition, windows]);

  useEffect(() => {
    if (!open) return;

    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (popoverRef.current?.contains(target)) return;
      if (triggerRef.current?.contains(target)) return;
      onClose();
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      onClose();
      triggerRef.current?.focus();
    };

    window.addEventListener("pointerdown", handlePointerDown);
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("pointerdown", handlePointerDown);
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [onClose, open, triggerRef]);

  if (!open) return null;

  return createPortal(
    <div
      ref={popoverRef}
      id={id}
      className={`agent-usage-popover agent-usage-item-${provider}`}
      role="dialog"
      aria-label={`${agent} subscription windows`}
      style={
        position
          ? { left: position.left, top: position.top }
          : { left: 0, top: 0, visibility: "hidden" }
      }
    >
      <div className="agent-usage-popover-heading">
        <span className="agent-usage-agent">
          <AgentMark provider={provider} size={16} />
          <span className="agent-usage-popover-title">{agent} usage</span>
        </span>
        <span className="agent-usage-popover-subtitle">All windows</span>
      </div>
      <div className="agent-usage-popover-windows">
        {windows.map((usageWindow) => (
          <UsageWindow
            key={usageWindow.label}
            agent={agent}
            estimate={estimate}
            nowSeconds={nowSeconds}
            window={usageWindow}
          />
        ))}
      </div>
      <div className="agent-usage-popover-source">Source: {source}</div>
    </div>,
    document.body,
  );
}

type UsageItemProps = {
  provider: AgentProviderId;
  snapshot?: AgentUsageSnapshot;
  nowSeconds: number;
  duplicate?: boolean;
};

function UsageItem({
  provider,
  snapshot,
  nowSeconds,
  duplicate = false,
}: UsageItemProps) {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const popoverId = useId();

  const windows = snapshot?.windows ?? [];
  const primaryWindow = primaryUsageWindow(windows);
  const connected = snapshot?.connected ?? false;
  const agent = usageAgentLabel(provider, snapshot);
  const estimate = snapshot?.source.toLowerCase().includes("estimate") ?? false;
  const status = !snapshot
    ? "Checking…"
    : !connected
      ? "Not connected"
      : windows.length === 0
        ? "Usage unavailable"
        : "";
  const error =
    snapshot?.error !== "Not connected" ? snapshot?.error ?? "" : "";

  return (
    <div
      className={`agent-usage-item agent-usage-item-${provider}${
        windows.length === 0 ? " is-empty" : ""
      }${!connected && snapshot ? " is-disconnected" : ""}`}
      role={duplicate ? undefined : "listitem"}
      aria-hidden={duplicate || undefined}
      title={
        duplicate || primaryWindow
          ? undefined
          : usageTitle(provider, nowSeconds, snapshot)
      }
    >
      <div className="agent-usage-primary">
        <span className="agent-usage-agent">
          <AgentMark provider={provider} size={14} />
          <span className="agent-usage-agent-name">{agent}</span>
        </span>
        {primaryWindow ? (
          <>
            <div className="agent-usage-summary">
              <UsageWindow
                agent={agent}
                estimate={estimate}
                nowSeconds={nowSeconds}
                window={primaryWindow}
              />
            </div>
            <span className="agent-usage-expand-glyph" aria-hidden>
              ⌃
            </span>
          </>
        ) : (
          <div className="agent-usage-state">
            {/* Track first: both share one grid cell, so the label must come
                after it to paint on top of the band. */}
            <span className="agent-usage-track" aria-hidden />
            <span className="agent-usage-percent">{status || " "}</span>
          </div>
        )}
      </div>
      {windows.length === 0 && error ? (
        <div className="agent-usage-error">{error}</div>
      ) : null}
      {primaryWindow && !duplicate ? (
        <>
          <button
            ref={triggerRef}
            type="button"
            className="agent-usage-trigger"
            aria-label={`Show all ${agent} subscription windows`}
            aria-haspopup="dialog"
            aria-expanded={open}
            aria-controls={popoverId}
            title={`Show all ${agent} subscription windows`}
            onClick={() => setOpen((current) => !current)}
          />
          <UsagePopover
            agent={agent}
            estimate={estimate}
            id={popoverId}
            nowSeconds={nowSeconds}
            onClose={() => setOpen(false)}
            open={open}
            provider={provider}
            source={snapshot?.source ?? "Unknown"}
            triggerRef={triggerRef}
            windows={windows}
          />
        </>
      ) : null}
    </div>
  );
}

export function AgentUsageBar({
  workflowKey,
  providers,
  usage,
  refreshing,
  onRefresh,
}: Props) {
  const [nowSeconds, setNowSeconds] = useState(() =>
    Math.floor(Date.now() / 1000),
  );
  useEffect(() => {
    if (providers.length === 0) return;
    const timer = window.setInterval(
      () => setNowSeconds(Math.floor(Date.now() / 1000)),
      60_000,
    );
    return () => window.clearInterval(timer);
  }, [providers.length]);

  const usageByProvider = useMemo(
    () => new Map(usage.map((snapshot) => [snapshot.provider, snapshot])),
    [usage],
  );
  const marquee = providers.length > 4;
  const subtitle =
    providers.length === 0
      ? "No agents in this workflow"
      : refreshing
        ? "Refreshing subscription limits"
        : "Subscription remaining";

  return (
    <footer className="agent-usage-bar" aria-label="Agent subscription usage">
      <div className="agent-usage-heading">
        <span className="agent-usage-title">Usage</span>
        <span className="agent-usage-subtitle">{subtitle}</span>
      </div>

      <div
        key={workflowKey}
        className="agent-usage-workflow-content"
        data-workflow-key={workflowKey}
      >
        {providers.length > 0 ? (
          <div
            className={`agent-usage-items${marquee ? " is-marquee" : ""}`}
            role="list"
            style={{
              "--usage-marquee-duration": `${Math.max(14, providers.length * 4)}s`,
            } as CSSProperties}
          >
            <div className="agent-usage-items-track">
              {providers.map((provider) => (
                <UsageItem
                  key={provider}
                  provider={provider}
                  snapshot={usageByProvider.get(provider)}
                  nowSeconds={nowSeconds}
                />
              ))}
              {marquee
                ? providers.map((provider) => (
                    <UsageItem
                      key={`${provider}-duplicate`}
                      provider={provider}
                      snapshot={usageByProvider.get(provider)}
                      nowSeconds={nowSeconds}
                      duplicate
                    />
                  ))
                : null}
            </div>
          </div>
        ) : (
          <span className="agent-usage-empty">
            Add an agent step to see its limits
          </span>
        )}
      </div>

      {providers.length > 0 ? (
        <button
          type="button"
          className={`agent-usage-refresh${refreshing ? " is-refreshing" : ""}`}
          onClick={onRefresh}
          disabled={refreshing}
          title="Refresh usage data"
          aria-label="Refresh usage data"
        >
          <RefreshGlyph />
        </button>
      ) : null}
    </footer>
  );
}
