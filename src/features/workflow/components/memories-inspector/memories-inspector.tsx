import { useEffect, useMemo, useRef, useState } from "react";
import { Modal, ModalHeader } from "../../../../components/modal";
import * as api from "../../api";
import {
  createHtmlReportPreview,
  extractHtmlReport,
} from "../../html-report";
import { useWorkflowStore } from "../../store";
import type { MemoryKind, OutputMemory } from "../../types";

type Filter = "all" | "pinned" | "linked" | MemoryKind;

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
  const addNote = useWorkflowStore((s) => s.addNote);
  const linkMemory = useWorkflowStore((s) => s.linkMemory);
  const unlinkMemory = useWorkflowStore((s) => s.unlinkMemory);
  const updateMemoryFields = useWorkflowStore((s) => s.updateMemoryFields);
  const togglePinMemory = useWorkflowStore((s) => s.togglePinMemory);
  const removeMemory = useWorkflowStore((s) => s.removeMemory);
  const clearMemories = useWorkflowStore((s) => s.clearMemories);

  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [kind, setKind] = useState<MemoryKind>("text");
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [creating, setCreating] = useState(false);
  const [linkable, setLinkable] = useState<OutputMemory[]>([]);
  const [showLinker, setShowLinker] = useState(false);
  const [viewSource, setViewSource] = useState(false);
  const htmlPreviewRef = useRef<HTMLIFrameElement>(null);

  const htmlPreview = useMemo(() => createHtmlReportPreview(body), [body]);

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
      if (!q) return true;
      return (
        m.title.toLowerCase().includes(q) ||
        m.body.toLowerCase().includes(q) ||
        m.source.toLowerCase().includes(q) ||
        (m.sourceWorkflowName ?? "").toLowerCase().includes(q)
      );
    });
  }, [memories, query, filter]);

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
    setDirty(true);
    setViewSource(true);
  };

  const saveSelected = async () => {
    if (!title.trim() && !creating) return;
    setSaving(true);
    try {
      if (creating) {
        await addNote(title.trim() || "Note", body);
        setCreating(false);
        const latest = useWorkflowStore.getState().memories[0];
        if (latest) setSelectedId(latest.id);
      } else if (selected) {
        await updateMemoryFields({
          id: selected.id,
          title: title.trim() || selected.title,
          body,
          kind,
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
                          {memory.kind}
                        </span>
                      )}
                    </span>
                    <span className="memories-list-preview">
                      {previewText(memory.body)}
                    </span>
                    <span className="memories-list-meta">
                      {memory.origin === "linked"
                        ? `Linked · ${formatWhen(memory.updatedAt || memory.createdAt)}`
                        : formatWhen(memory.updatedAt || memory.createdAt)}
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
                <p className="muted memories-detail-meta">
                  Source: {selected.source}
                  {selected.artifactPath
                    ? ` · Artifact on disk`
                    : ""}
                  {" · "}
                  Updated {formatWhen(selected.updatedAt || selected.createdAt)}
                </p>
              ) : (
                <p className="muted memories-detail-meta">
                  Notes are durable workflow context. Pin them to inject into
                  the next agent run.
                </p>
              )}
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
