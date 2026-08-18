import { useEffect, useMemo, useRef, useState } from "react";
import { Modal, ModalHeader } from "../../../../components/modal";
import * as api from "../../api";
import {
  createHtmlReportPreview,
  extractHtmlReport,
} from "../../html-report";
import { useWorkflowStore } from "../../store";
import { workspaceScopeAvailable } from "../../memories";
import type {
  MemoryKind,
  MemoryScopeType,
  MemoryStatus,
  MemoryType,
  OutputMemory,
} from "../../types";

type Filter = "all" | "pinned" | "linked" | MemoryKind;
type ScopeFilter = "all" | "user" | "workspace" | "workflow" | "inactive";

function formatWhen(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

function previewText(text: string, max = 100) {
  if (extractHtmlReport(text)) return "HTML report · Open to preview";
  const flat = text.replace(/\s+/g, " ").trim();
  if (flat.length <= max) return flat;
  return `${flat.slice(0, max)}…`;
}

function fitHtmlPreview(frame: HTMLIFrameElement | null) {
  const document = frame?.contentDocument;
  if (!frame || !document) return;

  frame.style.height = "1px";
  const height = Math.max(
    document.documentElement.scrollHeight,
    document.body?.scrollHeight ?? 0,
  );
  frame.style.height = `${Math.max(300, height)}px`;
}

type Props = {
  open: boolean;
  initialMemoryId?: string | null;
  onClose: () => void;
};

export function MemoriesInspector({ open, initialMemoryId, onClose }: Props) {
  const memories = useWorkflowStore((s) => s.memories);
  const activeWorkflowId = useWorkflowStore((s) => s.activeWorkflowId);
  const workflows = useWorkflowStore((s) => s.workflows);
  const addMemory = useWorkflowStore((s) => s.addMemory);
  const linkMemory = useWorkflowStore((s) => s.linkMemory);
  const unlinkMemory = useWorkflowStore((s) => s.unlinkMemory);
  const updateMemoryFields = useWorkflowStore((s) => s.updateMemoryFields);
  const togglePinMemory = useWorkflowStore((s) => s.togglePinMemory);
  const removeMemory = useWorkflowStore((s) => s.removeMemory);
  const clearMemories = useWorkflowStore((s) => s.clearMemories);

  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [scopeFilter, setScopeFilter] = useState<ScopeFilter>("all");
  const [typeFilter, setTypeFilter] = useState<MemoryType | "all">("all");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [kind, setKind] = useState<MemoryKind>("text");
  const [scopeType, setScopeType] = useState<MemoryScopeType>("workflow");
  const [memoryType, setMemoryType] = useState<MemoryType>("note");
  const [status, setStatus] = useState<MemoryStatus>("active");
  const [salience, setSalience] = useState(50);
  const [confidence, setConfidence] = useState(1);
  const [lastConfirmedAt, setLastConfirmedAt] = useState("");
  const [expiresAt, setExpiresAt] = useState("");
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [creating, setCreating] = useState(false);
  const [linkable, setLinkable] = useState<OutputMemory[]>([]);
  const [showLinker, setShowLinker] = useState(false);
  const [viewSource, setViewSource] = useState(false);
  const htmlPreviewRef = useRef<HTMLIFrameElement>(null);

  const htmlPreview = useMemo(() => createHtmlReportPreview(body), [body]);
  const activeWorkflow = workflows.find(({ id }) => id === activeWorkflowId);
  const hasWorkingDirectory = workspaceScopeAvailable(
    activeWorkflow?.workingDirectory,
  );

  useEffect(() => {
    if (!htmlPreview || viewSource) return;
    const resize = () => fitHtmlPreview(htmlPreviewRef.current);
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, [htmlPreview, viewSource]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return memories.filter((m) => {
      if (filter === "pinned" && !m.pinned) return false;
      if (filter === "linked" && m.origin !== "linked") return false;
      if (
        filter !== "all" &&
        filter !== "pinned" &&
        filter !== "linked" &&
        m.kind !== filter
      ) {
        return false;
      }
      if (scopeFilter === "inactive" && m.status === "active") return false;
      if (
        scopeFilter !== "all" &&
        scopeFilter !== "inactive" &&
        m.scopeType !== scopeFilter
      ) {
        return false;
      }
      if (typeFilter !== "all" && m.memoryType !== typeFilter) return false;
      if (!q) return true;
      return (
        m.title.toLowerCase().includes(q) ||
        m.body.toLowerCase().includes(q) ||
        m.source.toLowerCase().includes(q) ||
        (m.sourceWorkflowName ?? "").toLowerCase().includes(q)
      );
    });
  }, [memories, query, filter, scopeFilter, typeFilter]);

  const linkedCount = memories.filter((m) => m.origin === "linked").length;

  const selected =
    filtered.find((m) => m.id === selectedId) ??
    memories.find((m) => m.id === selectedId) ??
    null;
  const isLinkedSelected = selected?.origin === "linked";

  useEffect(() => {
    if (!open) return;
    const preferred =
      initialMemoryId && memories.some((m) => m.id === initialMemoryId)
        ? initialMemoryId
        : (memories[0]?.id ?? null);
    setSelectedId(preferred);
    setQuery("");
    setFilter("all");
    setScopeFilter("all");
    setTypeFilter("all");
    setCreating(false);
    setShowLinker(false);
    setViewSource(false);
  }, [open, initialMemoryId, activeWorkflowId]);

  useEffect(() => {
    if (!open || !activeWorkflowId || !showLinker) return;
    let cancelled = false;
    void api.listLinkableMemories(activeWorkflowId).then((rows) => {
      if (!cancelled) setLinkable(rows);
    });
    return () => {
      cancelled = true;
    };
  }, [open, activeWorkflowId, showLinker, memories.length]);

  useEffect(() => {
    if (!open) return;
    if (selectedId && !memories.some((m) => m.id === selectedId)) {
      setSelectedId(memories[0]?.id ?? null);
    }
  }, [open, memories, selectedId]);

  useEffect(() => {
    if (!selected || creating) return;
    setTitle(selected.title);
    setBody(selected.body);
    setKind(selected.kind);
    setScopeType(selected.scopeType);
    setMemoryType(selected.memoryType);
    setStatus(selected.status);
    setSalience(selected.salience);
    setConfidence(selected.confidence);
    setLastConfirmedAt(selected.lastConfirmedAt ?? "");
    setExpiresAt(selected.expiresAt ?? "");
    setDirty(false);
    setViewSource(false);
  }, [selected?.id, selected?.updatedAt, creating]);

  const loadMemory = (memory: OutputMemory) => {
    setCreating(false);
    setSelectedId(memory.id);
    setViewSource(false);
  };

  const startNewNote = () => {
    setCreating(true);
    setSelectedId(null);
    setTitle("");
    setBody("");
    setKind("note");
    setScopeType("workflow");
    setMemoryType("note");
    setStatus("active");
    setSalience(50);
    setConfidence(1);
    setLastConfirmedAt("");
    setExpiresAt("");
    setDirty(true);
    setViewSource(true);
  };

  const saveSelected = async () => {
    if (!title.trim() && !creating) return;
    setSaving(true);
    try {
      if (creating) {
        await addMemory({
          title: title.trim() || "Note",
          body,
          kind,
          scopeType,
          memoryType,
          source: "manual",
          salience,
          confidence,
          status,
          lastConfirmedAt: lastConfirmedAt || null,
          expiresAt: expiresAt || null,
        });
        setCreating(false);
        const latest = useWorkflowStore.getState().memories[0];
        if (latest) setSelectedId(latest.id);
      } else if (selected) {
        if (
          selected.scopeType !== scopeType &&
          !window.confirm(
            `Move this memory from ${selected.scopeLabel} scope to ${scopeType} scope?`,
          )
        ) {
          return;
        }
        await updateMemoryFields({
          id: selected.id,
          title: title.trim() || selected.title,
          body,
          kind,
          scopeType,
          memoryType,
          salience,
          confidence,
          status,
          lastConfirmedAt: lastConfirmedAt || null,
          expiresAt: expiresAt || null,
        });
        setDirty(false);
      }
    } finally {
      setSaving(false);
    }
  };

  const deleteSelected = async () => {
    if (!selected) return;
    const id = selected.id;
    await removeMemory(id);
  };

  const pinnedCount = memories.filter(
    (m) => m.pinned && m.origin !== "linked",
  ).length;
  const visiblePinnedBytes = memories
    .filter((memory) => memory.pinned && memory.status === "active")
    .reduce(
      (total, memory) =>
        total + new TextEncoder().encode(`${memory.title}\n${memory.body}`).length,
      0,
    );
  const supersedes = selected?.supersedesId
    ? memories.find(({ id }) => id === selected.supersedesId)
    : null;
  const supersededBy = selected
    ? memories.find(({ supersedesId }) => supersedesId === selected.id)
    : null;

  return (
    <Modal open={open} size="xl" onClose={onClose} label="Memories inspector">
      <ModalHeader
        strong
        titleAs="h2"
        title="Memories"
        description={
          memories.length === 0
            ? "No memories yet for this workflow."
            : `${memories.length} total${
                pinnedCount ? ` · ${pinnedCount} pinned for next run` : ""
              }${
                linkedCount
                  ? ` · ${linkedCount} linked from other workflows`
                  : ""
              }`
        }
        actions={
          <>
            <button
              type="button"
              className="ghost"
              onClick={() => setShowLinker((v) => !v)}
            >
              {showLinker ? "Hide linker" : "Link from workflow…"}
            </button>
            <button type="button" className="ghost" onClick={startNewNote}>
              New note
            </button>
            {memories.some((m) => m.origin !== "linked") ? (
              <button
                type="button"
                className="ghost danger"
                onClick={() => void clearMemories()}
              >
                Clear owned
              </button>
            ) : null}
            <button type="button" className="ghost" onClick={onClose}>
              Close
            </button>
          </>
        }
      />

      <div className="memories-inspector-body">
        <aside className="memories-inspector-list">
          <input
            type="search"
            className="memories-inspector-search"
            placeholder="Search memories…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <div className="memories-filter-row" role="tablist">
            {(
              [
                ["all", "All"],
                ["linked", "Linked"],
                ["pinned", "Pinned"],
                ["note", "Notes"],
                ["text", "Outputs"],
                ["artifact", "Artifacts"],
              ] as const
            ).map(([id, label]) => (
              <button
                key={id}
                type="button"
                role="tab"
                aria-selected={filter === id}
                className={`ghost memories-filter${
                  filter === id ? " is-active" : ""
                }`}
                onClick={() => setFilter(id)}
              >
                {label}
              </button>
            ))}
          </div>
          <div className="memories-filter-selects">
            <label className="field">
              <span>Scope</span>
              <select
                value={scopeFilter}
                onChange={(event) =>
                  setScopeFilter(event.target.value as ScopeFilter)
                }
              >
                <option value="all">All scopes</option>
                <option value="user">User</option>
                <option value="workspace">Workspace</option>
                <option value="workflow">Workflow</option>
                <option value="inactive">Inactive</option>
              </select>
            </label>
            <label className="field">
              <span>Type</span>
              <select
                value={typeFilter}
                onChange={(event) =>
                  setTypeFilter(event.target.value as MemoryType | "all")
                }
              >
                <option value="all">All types</option>
                {(
                  [
                    "preference",
                    "fact",
                    "decision",
                    "constraint",
                    "lesson",
                    "episode",
                    "checkpoint",
                    "note",
                    "output",
                    "artifact",
                  ] as MemoryType[]
                ).map((value) => (
                  <option key={value} value={value}>
                    {value}
                  </option>
                ))}
              </select>
            </label>
          </div>

          {showLinker ? (
            <div className="memories-linker">
              <p className="muted">
                Link an existing memory from another workflow into this one.
              </p>
              {linkable.length === 0 ? (
                <p className="muted memories-inspector-empty">
                  No other-workflow memories available.
                </p>
              ) : (
                <ul>
                  {linkable.map((memory) => (
                    <li key={memory.id}>
                      <button
                        type="button"
                        className="memories-list-item"
                        onClick={() =>
                          void linkMemory(memory.id).then((linked) => {
                            if (linked) {
                              setSelectedId(linked.id);
                              setShowLinker(false);
                              setCreating(false);
                            }
                          })
                        }
                      >
                        <span className="memories-list-top">
                          <span className="memories-list-title">
                            {memory.title}
                          </span>
                          <span className="memory-origin-badge">
                            From {memory.sourceWorkflowName ?? "workflow"}
                          </span>
                        </span>
                        <span className="memories-list-preview">
                          {previewText(memory.body)}
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          ) : null}

          {filtered.length === 0 ? (
            <p className="muted memories-inspector-empty">
              {memories.length === 0
                ? "Run a workflow, link from another flow, or add a note."
                : "Nothing matches this filter."}
            </p>
          ) : (
            <ul>
              {filtered.map((memory) => (
                <li key={memory.id}>
                  <button
                    type="button"
                    className={`memories-list-item${
                      memory.id === selectedId && !creating ? " is-active" : ""
                    }`}
                    onClick={() => loadMemory(memory)}
                  >
                    <span className="memories-list-top">
                      <span className="memories-list-title">
                        {memory.pinned && memory.origin !== "linked"
                          ? "★ "
                          : ""}
                        {memory.title}
                      </span>
                      {memory.origin === "linked" ? (
                        <span className="memory-origin-badge">
                          From {memory.sourceWorkflowName ?? "workflow"}
                        </span>
                      ) : (
                        <span className="memories-list-kind">
                          {memory.scopeType} · {memory.memoryType}
                        </span>
                      )}
                    </span>
                    <span className="memories-list-preview">
                      {previewText(memory.body)}
                    </span>
                    <span className="memories-list-meta">
                      {memory.status !== "active" ? `${memory.status} · ` : ""}
                      {memory.origin === "linked" ? "Linked · " : ""}
                      {formatWhen(memory.updatedAt || memory.createdAt)}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </aside>

        <section className="memories-inspector-detail">
          {creating || selected ? (
            <>
              {isLinkedSelected && selected ? (
                <div className="memory-linked-banner">
                  Comes from{" "}
                  <strong>
                    {selected.sourceWorkflowName ?? "another workflow"}
                  </strong>
                  . Read-only here — use a Memories node to inject it, or
                  unlink to remove the reference.
                </div>
              ) : null}
              <div className="memories-detail-toolbar">
                <label className="field memories-kind-field">
                  <span>Kind</span>
                  <select
                    value={kind}
                    disabled={creating || isLinkedSelected}
                    onChange={(e) => {
                      setKind(e.target.value as MemoryKind);
                      setDirty(true);
                    }}
                  >
                    <option value="note">Note</option>
                    <option value="text">Output</option>
                    <option value="artifact">Artifact</option>
                  </select>
                </label>
                <label className="field memories-kind-field">
                  <span>Scope</span>
                  <select
                    value={scopeType}
                    disabled={isLinkedSelected}
                    onChange={(event) => {
                      setScopeType(event.target.value as MemoryScopeType);
                      setDirty(true);
                    }}
                  >
                    <option value="workflow">Workflow</option>
                    <option value="workspace" disabled={!hasWorkingDirectory}>
                      Workspace
                    </option>
                    <option value="user">User</option>
                  </select>
                </label>
                <label className="field memories-kind-field">
                  <span>Type</span>
                  <select
                    value={memoryType}
                    disabled={isLinkedSelected}
                    onChange={(event) => {
                      setMemoryType(event.target.value as MemoryType);
                      setDirty(true);
                    }}
                  >
                    {(
                      [
                        "preference",
                        "fact",
                        "decision",
                        "constraint",
                        "lesson",
                        "episode",
                        "checkpoint",
                        "note",
                        "output",
                        "artifact",
                      ] as MemoryType[]
                    ).map((value) => (
                      <option key={value} value={value}>
                        {value}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="field memories-kind-field">
                  <span>Status</span>
                  <select
                    value={status}
                    disabled={isLinkedSelected}
                    onChange={(event) => {
                      const next = event.target.value as MemoryStatus;
                      setStatus(next);
                      setDirty(true);
                    }}
                  >
                    <option value="active">Active</option>
                    <option value="superseded">Superseded</option>
                    <option value="retracted">Retracted</option>
                  </select>
                </label>
                {htmlPreview ? (
                  <button
                    type="button"
                    className="ghost"
                    aria-pressed={viewSource}
                    onClick={() => setViewSource((current) => !current)}
                  >
                    {viewSource ? "Preview HTML" : "View source"}
                  </button>
                ) : null}
                {selected && !creating && !isLinkedSelected ? (
                  <button
                    type="button"
                    className={`ghost${selected.pinned ? " memories-pin-on" : ""}`}
                    disabled={selected.status !== "active"}
                    onClick={() => void togglePinMemory(selected.id)}
                  >
                    {selected.pinned ? "Unpin" : "Pin for run"}
                  </button>
                ) : null}
                {!isLinkedSelected ? (
                  <button
                    type="button"
                    className="primary"
                    disabled={saving || (!dirty && !creating) || !body.trim()}
                    onClick={() => void saveSelected()}
                  >
                    {saving ? "Saving…" : creating ? "Create note" : "Save"}
                  </button>
                ) : null}
                {selected && !creating ? (
                  <button
                    type="button"
                    className="ghost danger"
                    onClick={() =>
                      void (isLinkedSelected
                        ? unlinkMemory(selected.id)
                        : deleteSelected())
                    }
                  >
                    {isLinkedSelected ? "Unlink" : "Delete"}
                  </button>
                ) : null}
              </div>

              <label className="field">
                <span>Title</span>
                <input
                  type="text"
                  value={title}
                  placeholder="Memory title"
                  readOnly={isLinkedSelected}
                  onChange={(e) => {
                    setTitle(e.target.value);
                    setDirty(true);
                  }}
                />
              </label>

              <div className="memories-metadata-grid">
                <label className="field">
                  <span>Salience · {salience}</span>
                  <input
                    type="range"
                    min="0"
                    max="100"
                    value={salience}
                    disabled={isLinkedSelected}
                    onChange={(event) => {
                      setSalience(Number(event.target.value));
                      setDirty(true);
                    }}
                  />
                </label>
                <label className="field">
                  <span>Confidence · {Math.round(confidence * 100)}%</span>
                  <input
                    type="range"
                    min="0"
                    max="100"
                    value={Math.round(confidence * 100)}
                    disabled={isLinkedSelected}
                    onChange={(event) => {
                      setConfidence(Number(event.target.value) / 100);
                      setDirty(true);
                    }}
                  />
                </label>
                <label className="field">
                  <span>Last confirmed (RFC3339)</span>
                  <input
                    type="text"
                    placeholder="2026-08-18T10:00:00Z"
                    value={lastConfirmedAt}
                    readOnly={isLinkedSelected}
                    onChange={(event) => {
                      setLastConfirmedAt(event.target.value);
                      setDirty(true);
                    }}
                  />
                </label>
                <label className="field">
                  <span>Expiry (RFC3339)</span>
                  <input
                    type="text"
                    placeholder="No expiry"
                    value={expiresAt}
                    readOnly={isLinkedSelected}
                    onChange={(event) => {
                      setExpiresAt(event.target.value);
                      setDirty(true);
                    }}
                  />
                </label>
              </div>

              <div className="field memories-body-field">
                <span>Content</span>
                {htmlPreview && !viewSource ? (
                  <iframe
                    ref={htmlPreviewRef}
                    className="memories-html-preview"
                    title={`${title || "Memory"} preview`}
                    sandbox="allow-same-origin"
                    referrerPolicy="no-referrer"
                    scrolling="no"
                    srcDoc={htmlPreview}
                    onLoad={(event) => fitHtmlPreview(event.currentTarget)}
                  />
                ) : (
                  <textarea
                    aria-label="Content"
                    value={body}
                    rows={16}
                    placeholder="Memory content…"
                    readOnly={isLinkedSelected}
                    onChange={(e) => {
                      setBody(e.target.value);
                      setDirty(true);
                      setViewSource(true);
                    }}
                  />
                )}
              </div>

              {selected && !creating ? (
                <div className="muted memories-detail-meta">
                  <p>
                    Scope: {selected.scopeLabel} · Type: {selected.memoryType} ·
                    Source: {selected.source}
                    {selected.sourceWorkflowName
                      ? ` from ${selected.sourceWorkflowName}`
                      : ""}
                    {selected.nodeId ? ` · Node ${selected.nodeId}` : ""}
                    {selected.artifactPath ? " · Artifact on disk" : ""}
                  </p>
                  {selected.runId ? (
                    <button
                      type="button"
                      className="ghost memories-history-link"
                      onClick={() => {
                        document
                          .querySelector<HTMLButtonElement>(
                            'button[aria-label="History"]',
                          )
                          ?.click();
                        onClose();
                      }}
                    >
                      Open run {selected.runId} in History
                    </button>
                  ) : null}
                  {supersedes ? <p>Supersedes {supersedes.title}</p> : null}
                  {supersededBy ? (
                    <p>Superseded by {supersededBy.title}</p>
                  ) : null}
                  <p>
                    Updated {formatWhen(selected.updatedAt || selected.createdAt)}
                  </p>
                </div>
              ) : (
                <p className="muted memories-detail-meta">
                  Notes are durable workflow context. Pin them to inject into
                  the next agent run.
                </p>
              )}
              <div className="memory-budget-note" role="status">
                Pinned context is selected deterministically within a 6,000-byte
                budget (User 1,500 · Workspace 2,000 · Workflow/linked 2,500).
                {visiblePinnedBytes > 6_000 ? (
                  <strong>
                    {" "}Visible pins exceed the budget; overflow remains in the
                    library and will be omitted from the run prompt.
                  </strong>
                ) : null}
              </div>
            </>
          ) : (
            <div className="memories-inspector-placeholder">
              <h3>Select a memory</h3>
              <p className="muted">
                Edit titles, pin context for runs, or delete what you don’t
                need.
              </p>
              <button type="button" className="primary" onClick={startNewNote}>
                New note
              </button>
            </div>
          )}
        </section>
      </div>
    </Modal>
  );
}
