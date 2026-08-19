import { confirm as confirmDialog } from "@tauri-apps/plugin-dialog";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import { Icon } from "../../../../components/icon";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
  MenuItem,
  MenuSeparator,
} from "../../../../components/menu";
import { Modal, ModalHeader } from "../../../../components/modal";
import * as api from "../../api";
import {
  createHtmlReportPreview,
} from "../../html-report";
import { useWorkflowStore } from "../../store";
import type { MemoryKind, OutputMemory } from "../../types";
import {
  filterAndSortMemories,
  memorySearchSnippet,
  type MemoryKindFilter,
  type MemoryQuickFilter,
} from "./memory-list-model";

const KIND_LABELS: Record<MemoryKind, string> = {
  note: "Note",
  text: "Output",
  artifact: "Artifact",
};

function formatWhen(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

function highlightText(text: string, query: string): ReactNode {
  const q = query.trim();
  if (!q) return text;
  const index = text.toLowerCase().indexOf(q.toLowerCase());
  if (index < 0) return text;
  return (
    <>
      {text.slice(0, index)}
      <mark>{text.slice(index, index + q.length)}</mark>
      {text.slice(index + q.length)}
    </>
  );
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
  const [quickFilter, setQuickFilter] = useState<MemoryQuickFilter>("all");
  const [kindFilter, setKindFilter] = useState<MemoryKindFilter>("all");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [editing, setEditing] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [creating, setCreating] = useState(false);
  const [linkable, setLinkable] = useState<OutputMemory[]>([]);
  const [showLinker, setShowLinker] = useState(false);
  const [linkerQuery, setLinkerQuery] = useState("");
  const [linkerLoading, setLinkerLoading] = useState(false);
  const [linkerError, setLinkerError] = useState<string | null>(null);
  const [linkingId, setLinkingId] = useState<string | null>(null);
  const [viewSource, setViewSource] = useState(false);
  const [htmlExpanded, setHtmlExpanded] = useState(false);
  const [showHeaderMenu, setShowHeaderMenu] = useState(false);
  const [showDetailMenu, setShowDetailMenu] = useState(false);
  const [detailOpen, setDetailOpen] = useState(false);
  const htmlPreviewRef = useRef<HTMLIFrameElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const detailRef = useRef<HTMLElement>(null);
  const linkerSearchRef = useRef<HTMLInputElement>(null);

  const htmlPreview = useMemo(() => createHtmlReportPreview(body), [body]);

  useEffect(() => {
    if (!htmlPreview || viewSource) return;
    const resize = () => fitHtmlPreview(htmlPreviewRef.current);
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, [htmlPreview, viewSource]);

  const filtered = useMemo(() => {
    return filterAndSortMemories(memories, query, quickFilter, kindFilter);
  }, [memories, query, quickFilter, kindFilter]);

  const filteredLinkable = useMemo(() => {
    const q = linkerQuery.trim().toLowerCase();
    if (!q) return linkable;
    return linkable.filter(
      (memory) =>
        memory.title.toLowerCase().includes(q) ||
        memory.body.toLowerCase().includes(q) ||
        (memory.sourceWorkflowName ?? "").toLowerCase().includes(q),
    );
  }, [linkable, linkerQuery]);

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
    setQuickFilter("all");
    setKindFilter("all");
    setEditing(false);
    setCreating(false);
    setShowLinker(false);
    setViewSource(false);
    setHtmlExpanded(false);
    setDetailOpen(Boolean(initialMemoryId));
  }, [open, initialMemoryId, activeWorkflowId]);

  useEffect(() => {
    if (!open || !activeWorkflowId || !showLinker) return;
    let cancelled = false;
    setLinkerLoading(true);
    setLinkerError(null);
    void api
      .listLinkableMemories(activeWorkflowId)
      .then((rows) => {
        if (!cancelled) setLinkable(rows);
      })
      .catch((error) => {
        if (!cancelled) setLinkerError(String(error));
      })
      .finally(() => {
        if (!cancelled) setLinkerLoading(false);
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
    if (!selected || creating || editing) return;
    setTitle(selected.title);
    setBody(selected.body);
    setDirty(false);
    setViewSource(false);
    setHtmlExpanded(false);
  }, [selected?.id, selected?.updatedAt, creating, editing]);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const isTyping =
        target?.tagName === "INPUT" ||
        target?.tagName === "TEXTAREA" ||
        target?.tagName === "SELECT" ||
        target?.isContentEditable;
      if (event.key === "/" && !isTyping) {
        event.preventDefault();
        searchRef.current?.focus();
      } else if (event.key === "Escape" && showLinker) {
        event.stopPropagation();
        setShowLinker(false);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, showLinker]);

  useEffect(() => {
    if (showLinker) {
      requestAnimationFrame(() => linkerSearchRef.current?.focus());
    }
  }, [showLinker]);

  const confirmDiscard = async () => {
    if (!dirty || (!editing && !creating)) return true;
    return confirmDialog("Discard your unsaved changes?", {
      title: "Unsaved changes",
      kind: "warning",
    });
  };

  const requestClose = async () => {
    if (!(await confirmDiscard())) return;
    onClose();
  };

  const loadMemory = async (memory: OutputMemory) => {
    if (!(await confirmDiscard())) return;
    setEditing(false);
    setCreating(false);
    setSelectedId(memory.id);
    setViewSource(false);
    setHtmlExpanded(false);
    setDetailOpen(true);
  };

  const startNewNote = async () => {
    if (!(await confirmDiscard())) return;
    setEditing(true);
    setCreating(true);
    setSelectedId(null);
    setTitle("");
    setBody("");
    setDirty(false);
    setViewSource(true);
    setHtmlExpanded(false);
    setDetailOpen(true);
  };

  const saveSelected = async () => {
    if (!title.trim() && !creating) return;
    setSaving(true);
    try {
      if (creating) {
        const previousIds = new Set(memories.map((memory) => memory.id));
        await addNote(title.trim() || "Note", body);
        setCreating(false);
        const created = useWorkflowStore
          .getState()
          .memories.find((memory) => !previousIds.has(memory.id));
        if (created) setSelectedId(created.id);
      } else if (selected) {
        await updateMemoryFields({
          id: selected.id,
          title: title.trim() || selected.title,
          body,
        });
      }
      setEditing(false);
      setDirty(false);
      setViewSource(false);
      setHtmlExpanded(false);
    } finally {
      setSaving(false);
    }
  };

  const cancelEditing = async () => {
    if (!(await confirmDiscard())) return;
    setEditing(false);
    setCreating(false);
    setDirty(false);
    if (selected) {
      setTitle(selected.title);
      setBody(selected.body);
    } else {
      setDetailOpen(false);
    }
  };

  const deleteSelected = async () => {
    if (!selected) return;
    const confirmed = await confirmDialog(
      `Delete “${selected.title}”? This cannot be undone.`,
      { title: "Delete memory", kind: "warning" },
    );
    if (!confirmed) return;
    const id = selected.id;
    await removeMemory(id);
  };

  const unlinkSelected = async () => {
    if (!selected) return;
    const confirmed = await confirmDialog(
      `Unlink “${selected.title}” from this workflow? The original memory will remain available.`,
      { title: "Unlink memory", kind: "warning" },
    );
    if (!confirmed) return;
    await unlinkMemory(selected.id);
  };

  const clearOwned = async () => {
    if (!(await confirmDiscard())) return;
    const ownedCount = memories.filter(
      (memory) => memory.origin !== "linked",
    ).length;
    const confirmed = await confirmDialog(
      `Delete ${ownedCount} owned ${
        ownedCount === 1 ? "memory" : "memories"
      }? Linked memories will remain.`,
      { title: "Clear owned memories", kind: "warning" },
    );
    if (!confirmed) return;
    await clearMemories();
  };

  const openLinker = () => {
    setLinkerQuery("");
    setShowLinker(true);
  };

  const linkSelectedMemory = async (memory: OutputMemory) => {
    setLinkingId(memory.id);
    try {
      const linked = await linkMemory(memory.id);
      if (linked) {
        setSelectedId(linked.id);
        setShowLinker(false);
        setCreating(false);
        setEditing(false);
        setDetailOpen(true);
      }
    } finally {
      setLinkingId(null);
    }
  };

  const handleListKeyDown = (event: ReactKeyboardEvent<HTMLElement>) => {
    if (!filtered.length) return;
    const currentIndex = filtered.findIndex(
      (memory) => memory.id === selectedId,
    );
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const direction = event.key === "ArrowDown" ? 1 : -1;
      const nextIndex =
        currentIndex < 0
          ? 0
          : (currentIndex + direction + filtered.length) % filtered.length;
      void loadMemory(filtered[nextIndex]);
    } else if (event.key === "Enter" && selected) {
      event.preventDefault();
      setDetailOpen(true);
      detailRef.current?.focus();
    }
  };

  const pinnedCount = memories.filter(
    (m) => m.pinned && m.origin !== "linked",
  ).length;

  return (
    <Modal
      open={open}
      size="xl"
      className={`memories-inspector-modal${
        htmlExpanded ? " is-html-expanded" : ""
      }`}
      onClose={() => void requestClose()}
      label="Memories inspector"
      closeOnEscape={!showLinker}
    >
      <ModalHeader
        strong
        titleAs="h2"
        title="Memories"
        description={
          memories.length === 0 ? (
            "No memories yet for this workflow."
          ) : (
            <span className="memories-header-stats">
              <span>{memories.length} memories</span>
              {pinnedCount ? <span>{pinnedCount} in next run</span> : null}
              {linkedCount ? <span>{linkedCount} linked</span> : null}
            </span>
          )
        }
        actions={
          <>
            <button type="button" className="ghost" onClick={openLinker}>
              <Icon name="link" size={15} />
              Link memory
            </button>
            <button
              type="button"
              className="ghost"
              onClick={() => void startNewNote()}
            >
              <Icon name="plus" size={15} />
              New note
            </button>
            <DropdownMenu
              open={showHeaderMenu}
              onOpenChange={setShowHeaderMenu}
            >
              <DropdownMenuTrigger className="ghost memories-more-button">
                More
                <Icon name="caret-down" size={12} />
              </DropdownMenuTrigger>
              <DropdownMenuContent aria-label="Memory library actions">
                <MenuItem
                  danger
                  disabled={!memories.some((m) => m.origin !== "linked")}
                  onSelect={() => setShowHeaderMenu(false)}
                  onClick={() => void clearOwned()}
                >
                  Clear owned memories
                </MenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <button
              type="button"
              className="ghost modal-close-button"
              aria-label="Close"
              onClick={() => void requestClose()}
            >
              <Icon name="x" size={16} />
            </button>
          </>
        }
      />

      <div
        className={`memories-inspector-body${
          detailOpen ? " is-detail-open" : ""
        }`}
      >
        <aside
          className="memories-inspector-list"
          aria-label="Memory library"
          onKeyDown={handleListKeyDown}
        >
          <div className="memories-search-wrap">
            <Icon name="magnifying-glass" size={15} />
            <input
              ref={searchRef}
              type="search"
              className="memories-inspector-search"
              aria-label="Search memories"
              placeholder="Search memories"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
            <kbd>/</kbd>
          </div>
          <div className="memories-filter-row" role="tablist">
            {(
              [
                ["all", "All"],
                ["pinned", "Next run"],
                ["linked", "Linked"],
              ] as const
            ).map(([id, label]) => (
              <button
                key={id}
                type="button"
                role="tab"
                aria-selected={quickFilter === id}
                className={`ghost memories-filter${
                  quickFilter === id ? " is-active" : ""
                }`}
                onClick={() => setQuickFilter(id)}
              >
                {label}
              </button>
            ))}
            <select
              className="memories-kind-filter"
              aria-label="Filter by memory kind"
              value={kindFilter}
              onChange={(event) =>
                setKindFilter(event.target.value as MemoryKindFilter)
              }
            >
              <option value="all">All kinds</option>
              <option value="note">Notes</option>
              <option value="text">Outputs</option>
              <option value="artifact">Artifacts</option>
            </select>
          </div>

          {filtered.length === 0 ? (
            <div className="memories-list-empty">
              <Icon name="note" size={22} />
              <p>
                {memories.length === 0
                  ? "Run a workflow, link a memory, or create a note."
                  : "No memories match your search and filters."}
              </p>
              {memories.length === 0 ? (
                <button
                  type="button"
                  className="ghost"
                  onClick={() => void startNewNote()}
                >
                  New note
                </button>
              ) : null}
            </div>
          ) : (
            <ul className="memories-results">
              {filtered.map((memory) => (
                <li key={memory.id}>
                  <button
                    type="button"
                    className={`memories-list-item${
                      memory.id === selectedId && !creating ? " is-active" : ""
                    }`}
                    onClick={() => void loadMemory(memory)}
                  >
                    <span className="memories-list-top">
                      <span className="memories-list-title">
                        {memory.pinned && memory.origin !== "linked" ? (
                          <Icon
                            name="push-pin"
                            size={12}
                            className="memories-pin-icon"
                          />
                        ) : null}
                        <span className="memories-list-title-text">
                          {highlightText(memory.title, query)}
                        </span>
                      </span>
                      <span className="memories-list-kind">
                        {KIND_LABELS[memory.kind]}
                      </span>
                    </span>
                    <span className="memories-list-preview">
                      {highlightText(memorySearchSnippet(memory, query), query)}
                    </span>
                    <span className="memories-list-meta-row">
                      <span>
                        {memory.origin === "linked"
                          ? `From ${memory.sourceWorkflowName ?? "workflow"}`
                          : memory.source === "manual"
                            ? "Created here"
                            : "Workflow output"}
                      </span>
                      <time>
                        {formatWhen(memory.updatedAt || memory.createdAt)}
                      </time>
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </aside>

        <section
          ref={detailRef}
          className="memories-inspector-detail"
          tabIndex={-1}
        >
          {creating || selected ? (
            <>
              <button
                type="button"
                className="ghost memories-detail-back"
                onClick={() => setDetailOpen(false)}
              >
                <Icon name="arrow-left" size={15} />
                Memories
              </button>

              <header className="memories-detail-header">
                <div className="memories-detail-heading">
                  {selected?.origin === "linked" ? (
                    <div className="memories-detail-provenance">
                      <span className="memories-linked-source">
                        <Icon name="link" size={13} />
                        From {selected.sourceWorkflowName ?? "another workflow"}
                      </span>
                    </div>
                  ) : null}
                  {!editing && selected ? (
                    <h3>{selected.title}</h3>
                  ) : (
                    <h3>{creating ? "New note" : "Edit memory"}</h3>
                  )}
                  {selected ? (
                    <p className="muted">
                      Updated{" "}
                      {formatWhen(selected.updatedAt || selected.createdAt)}
                      {selected.artifactPath ? ". Artifact stored on disk." : ""}
                    </p>
                  ) : (
                    <p className="muted">
                      Notes remain available as workflow context.
                    </p>
                  )}
                </div>

                <div className="memories-detail-actions">
                  {htmlPreview && !editing ? (
                    <button
                      type="button"
                      className="ghost memories-expand-button"
                      aria-pressed={htmlExpanded}
                      aria-label={
                        htmlExpanded
                          ? "Exit expanded HTML preview"
                          : "Expand HTML preview"
                      }
                      title={
                        htmlExpanded
                          ? "Exit expanded preview"
                          : "Expand preview"
                      }
                      onClick={() => setHtmlExpanded((current) => !current)}
                    >
                      <Icon name="corners-out" size={17} />
                    </button>
                  ) : null}
                  {selected && !creating && !editing && !isLinkedSelected ? (
                    <button
                      type="button"
                      className={
                        selected.pinned ? "primary" : "ghost context-action"
                      }
                      onClick={() => void togglePinMemory(selected.id)}
                    >
                      <Icon name="push-pin" size={15} />
                      {selected.pinned
                        ? "Remove from next run"
                        : "Add to next run"}
                    </button>
                  ) : null}
                  {selected && !editing && !isLinkedSelected ? (
                    <button
                      type="button"
                      className="ghost"
                      onClick={() => setEditing(true)}
                    >
                      <Icon name="pencil-simple" size={15} />
                      Edit
                    </button>
                  ) : null}
                  {selected && !creating && !editing ? (
                    <DropdownMenu
                      open={showDetailMenu}
                      onOpenChange={setShowDetailMenu}
                    >
                      <DropdownMenuTrigger className="ghost memories-more-button">
                        More
                        <Icon name="caret-down" size={12} />
                      </DropdownMenuTrigger>
                      <DropdownMenuContent aria-label="Selected memory actions">
                        {htmlPreview ? (
                          <>
                            <MenuItem
                              onSelect={() => setShowDetailMenu(false)}
                              onClick={() =>
                                setViewSource((current) => !current)
                              }
                            >
                              {viewSource
                                ? "Show rendered preview"
                                : "View source"}
                            </MenuItem>
                            <MenuSeparator />
                          </>
                        ) : null}
                        <MenuItem
                          danger
                          onSelect={() => setShowDetailMenu(false)}
                          onClick={() =>
                            void (isLinkedSelected
                              ? unlinkSelected()
                              : deleteSelected())
                          }
                        >
                          {isLinkedSelected ? "Unlink memory" : "Delete memory"}
                        </MenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  ) : null}
                </div>
              </header>

              {editing ? (
                <div className="memories-edit-form">
                  <label className="field">
                    <span>Title</span>
                    <input
                      type="text"
                      value={title}
                      placeholder="Memory title"
                      autoFocus
                      onChange={(event) => {
                        setTitle(event.target.value);
                        setDirty(true);
                      }}
                    />
                  </label>
                  <label className="field memories-body-field">
                    <span>Content</span>
                    <textarea
                      aria-label="Content"
                      value={body}
                      rows={16}
                      placeholder="Write a durable note"
                      onChange={(event) => {
                        setBody(event.target.value);
                        setDirty(true);
                      }}
                    />
                  </label>
                  <div className="memories-edit-actions">
                    <button
                      type="button"
                      className="ghost"
                      disabled={saving}
                      onClick={() => void cancelEditing()}
                    >
                      Cancel
                    </button>
                    <button
                      type="button"
                      className="primary"
                      disabled={
                        saving || (!dirty && !creating) || !body.trim()
                      }
                      onClick={() => void saveSelected()}
                    >
                      {saving
                        ? "Saving…"
                        : creating
                          ? "Create note"
                          : "Save changes"}
                    </button>
                  </div>
                </div>
              ) : htmlPreview && !viewSource ? (
                <div className="memories-content-surface is-html">
                  <iframe
                    ref={htmlPreviewRef}
                    className="memories-html-preview"
                    title={`${selected?.title || "Memory"} preview`}
                    sandbox="allow-same-origin"
                    referrerPolicy="no-referrer"
                    scrolling="no"
                    srcDoc={htmlPreview}
                    onLoad={(event) => fitHtmlPreview(event.currentTarget)}
                  />
                </div>
              ) : (
                <div className="memories-content-surface">
                  <pre>{body}</pre>
                </div>
              )}
            </>
          ) : (
            <div className="memories-inspector-placeholder">
              <Icon name="note" size={28} />
              <h3>Select a memory</h3>
              <p className="muted">
                Choose an item to read it, add it to the next run, or edit its
                content.
              </p>
              <button
                type="button"
                className="primary"
                onClick={() => void startNewNote()}
              >
                New note
              </button>
            </div>
          )}
        </section>
      </div>

      {showLinker ? (
        <div
          className="memories-link-picker-backdrop"
          role="presentation"
          onMouseDown={() => setShowLinker(false)}
        >
          <section
            className="memories-link-picker"
            role="dialog"
            aria-modal="true"
            aria-labelledby="link-memory-title"
            onMouseDown={(event) => event.stopPropagation()}
          >
            <header>
              <div>
                <h3 id="link-memory-title">Link a memory</h3>
                <p className="muted">
                  Reuse context from another workflow without copying it.
                </p>
              </div>
              <button
                type="button"
                className="ghost modal-close-button"
                aria-label="Close link picker"
                onClick={() => setShowLinker(false)}
              >
                <Icon name="x" size={16} />
              </button>
            </header>
            <div className="memories-search-wrap">
              <Icon name="magnifying-glass" size={15} />
              <input
                ref={linkerSearchRef}
                type="search"
                aria-label="Search linkable memories"
                placeholder="Search other workflows"
                value={linkerQuery}
                onChange={(event) => setLinkerQuery(event.target.value)}
              />
            </div>
            <div className="memories-link-picker-results">
              {linkerLoading ? (
                <div className="memories-link-picker-loading" aria-label="Loading">
                  <span />
                  <span />
                  <span />
                </div>
              ) : linkerError ? (
                <p className="memories-link-picker-error">
                  Could not load memories. {linkerError}
                </p>
              ) : filteredLinkable.length === 0 ? (
                <div className="memories-list-empty">
                  <Icon name="link" size={22} />
                  <p>
                    {linkable.length === 0
                      ? "No memories from other workflows are available."
                      : "No linkable memories match your search."}
                  </p>
                </div>
              ) : (
                <ul>
                  {filteredLinkable.map((memory) => (
                    <li key={memory.id}>
                      <button
                        type="button"
                        className="memories-link-picker-item"
                        disabled={linkingId !== null}
                        onClick={() => void linkSelectedMemory(memory)}
                      >
                        <span>
                          <strong>
                            {highlightText(memory.title, linkerQuery)}
                          </strong>
                          <small>
                            {memory.sourceWorkflowName ?? "Another workflow"}
                          </small>
                        </span>
                        <span className="memories-list-preview">
                          {highlightText(
                            memorySearchSnippet(memory, linkerQuery),
                            linkerQuery,
                          )}
                        </span>
                        <span className="memories-link-picker-action">
                          {linkingId === memory.id ? "Linking…" : "Link"}
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </section>
        </div>
      ) : null}
    </Modal>
  );
}
