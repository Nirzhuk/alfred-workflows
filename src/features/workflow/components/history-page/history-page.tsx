import { useEffect, useMemo, useRef, useState } from "react";
import * as api from "../../api";
import type {
  HistorySearchHit,
  RunHistoryDetail,
  RunHistoryItem,
} from "../../types";
import {
  formatHistoryJson,
  formatHistoryWhen,
  historyHitLabel,
  historyMode,
  historyWorkflowId,
  isCurrentHistoryGeneration,
  literalHistorySnippet,
  type HistoryScope,
} from "./history-format";

const PAGE_SIZE = 25;

type Props = {
  activeWorkflowId: string | null;
  initialRunId?: string | null;
  onSelectedRunIdChange?: (runId: string | null) => void;
  onClose: () => void;
};

export function HistoryPage({
  activeWorkflowId,
  initialRunId = null,
  onSelectedRunIdChange,
  onClose,
}: Props) {
  const [query, setQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [scope, setScope] = useState<HistoryScope>(
    activeWorkflowId ? "current" : "all",
  );
  const [runs, setRuns] = useState<RunHistoryItem[]>([]);
  const [hits, setHits] = useState<HistorySearchHit[]>([]);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(
    initialRunId,
  );
  const [detail, setDetail] = useState<RunHistoryDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [hasMore, setHasMore] = useState(false);
  const [error, setError] = useState(false);
  const requestGeneration = useRef(0);
  const detailGeneration = useRef(0);

  useEffect(() => {
    const timeout = window.setTimeout(() => setDebouncedQuery(query), 250);
    return () => window.clearTimeout(timeout);
  }, [query]);

  useEffect(() => {
    if (!activeWorkflowId && scope === "current") setScope("all");
  }, [activeWorkflowId, scope]);

  useEffect(() => {
    setSelectedRunId(initialRunId);
  }, [initialRunId]);

  useEffect(
    () => () => {
      requestGeneration.current += 1;
      detailGeneration.current += 1;
    },
    [],
  );

  const workflowId = historyWorkflowId(scope, activeWorkflowId);
  const mode = historyMode(debouncedQuery);

  useEffect(() => {
    const generation = ++requestGeneration.current;
    let cancelled = false;
    setLoading(true);
    setLoadingMore(false);
    setError(false);

    const request =
      mode === "browse"
        ? api
            .listRunHistory({ workflowId, limit: PAGE_SIZE, offset: 0 })
            .then((rows) => {
              if (
                cancelled ||
                !isCurrentHistoryGeneration(
                  generation,
                  requestGeneration.current,
                )
              ) {
                return;
              }
              setRuns(rows);
              setHits([]);
              setHasMore(rows.length === PAGE_SIZE);
            })
        : api
            .searchHistory({
              query: debouncedQuery.trim(),
              workflowId,
              limit: 50,
            })
            .then((rows) => {
              if (
                cancelled ||
                !isCurrentHistoryGeneration(
                  generation,
                  requestGeneration.current,
                )
              ) {
                return;
              }
              setHits(rows);
              setRuns([]);
              setHasMore(false);
            });

    void request
      .catch(() => {
        if (
          !cancelled &&
          isCurrentHistoryGeneration(generation, requestGeneration.current)
        ) {
          setError(true);
        }
      })
      .finally(() => {
        if (
          !cancelled &&
          isCurrentHistoryGeneration(generation, requestGeneration.current)
        ) {
          setLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [debouncedQuery, mode, workflowId]);

  const loadMore = async () => {
    if (loadingMore || !hasMore || mode !== "browse") return;
    const generation = requestGeneration.current;
    setLoadingMore(true);
    setError(false);
    try {
      const rows = await api.listRunHistory({
        workflowId,
        limit: PAGE_SIZE,
        offset: runs.length,
      });
      if (!isCurrentHistoryGeneration(generation, requestGeneration.current)) {
        return;
      }
      setRuns((current) => [...current, ...rows]);
      setHasMore(rows.length === PAGE_SIZE);
    } catch {
      if (isCurrentHistoryGeneration(generation, requestGeneration.current)) {
        setError(true);
      }
    } finally {
      if (isCurrentHistoryGeneration(generation, requestGeneration.current)) {
        setLoadingMore(false);
      }
    }
  };

  useEffect(() => {
    if (!selectedRunId) {
      detailGeneration.current += 1;
      setLoadingDetail(false);
      setDetail(null);
      return;
    }
    const generation = ++detailGeneration.current;
    let cancelled = false;
    setLoadingDetail(true);
    setError(false);
    void api
      .getRunHistory(selectedRunId)
      .then((next) => {
        if (!cancelled && generation === detailGeneration.current) {
          setDetail(next);
        }
      })
      .catch(() => {
        if (!cancelled && generation === detailGeneration.current) {
          setError(true);
        }
      })
      .finally(() => {
        if (!cancelled && generation === detailGeneration.current) {
          setLoadingDetail(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [selectedRunId]);

  const openRun = (runId: string) => {
    setSelectedRunId(runId);
    onSelectedRunIdChange?.(runId);
  };

  const groupedHits = useMemo(
    () => ({
      runSteps: hits.filter((hit) => hit.kind === "run_step"),
      memories: hits.filter((hit) => hit.kind === "memory"),
    }),
    [hits],
  );

  return (
    <section className="history-page" aria-labelledby="history-title">
      <header className="history-page-header">
        <div>
          <p className="history-kicker">Private local data</p>
          <h1 id="history-title">History</h1>
          <p>Browse persisted runs or search run steps and saved memories.</p>
        </div>
        <button type="button" className="ghost" onClick={onClose}>
          Back to canvas
        </button>
      </header>

      <div className="history-page-body">
        <div className="history-toolbar">
          <label className="history-search">
            <span className="sr-only">Search history</span>
            <input
              type="search"
              value={query}
              placeholder="Search prompts, outputs, and memories"
              onChange={(event) => setQuery(event.target.value)}
            />
          </label>
          <div className="history-scope" role="group" aria-label="History scope">
            {activeWorkflowId ? (
              <button
                type="button"
                className={scope === "current" ? "is-active" : ""}
                aria-pressed={scope === "current"}
                onClick={() => setScope("current")}
              >
                Current workflow
              </button>
            ) : null}
            <button
              type="button"
              className={scope === "all" ? "is-active" : ""}
              aria-pressed={scope === "all"}
              onClick={() => setScope("all")}
            >
              All workflows
            </button>
          </div>
        </div>

        {error ? (
          <div className="history-message is-error" role="alert">
            History couldn’t be loaded. Try again.
          </div>
        ) : null}

        {detail ? (
          <RunDetail
            detail={detail}
            onBack={() => {
              setSelectedRunId(null);
              onSelectedRunIdChange?.(null);
            }}
          />
        ) : loadingDetail ? (
          <div className="history-message" role="status">
            Loading run detail…
          </div>
        ) : loading ? (
          <div className="history-message" role="status">
            {mode === "browse" ? "Loading recent runs…" : "Searching history…"}
          </div>
        ) : mode === "browse" ? (
          <RunBrowser
            runs={runs}
            hasMore={hasMore}
            loadingMore={loadingMore}
            onOpenRun={openRun}
            onLoadMore={loadMore}
          />
        ) : (
          <SearchResults
            runSteps={groupedHits.runSteps}
            memories={groupedHits.memories}
            onOpenRun={openRun}
          />
        )}
      </div>
    </section>
  );
}

function RunBrowser({
  runs,
  hasMore,
  loadingMore,
  onOpenRun,
  onLoadMore,
}: {
  runs: RunHistoryItem[];
  hasMore: boolean;
  loadingMore: boolean;
  onOpenRun: (runId: string) => void;
  onLoadMore: () => void;
}) {
  if (runs.length === 0) {
    return <div className="history-message">No persisted runs in this scope.</div>;
  }
  return (
    <div className="history-results">
      <div className="history-section-heading">
        <h2>Newest runs</h2>
        <span>{runs.length} loaded</span>
      </div>
      <ul className="history-list">
        {runs.map((run) => (
          <li key={run.id}>
            <button
              type="button"
              className="history-card"
              onClick={() => onOpenRun(run.id)}
            >
              <span className="history-card-topline">
                <strong>{run.workflowName}</strong>
                <span className={`history-status is-${run.status}`}>{run.status}</span>
              </span>
              <span className="history-card-meta">
                {formatHistoryWhen(run.createdAt)} · {run.trigger} · {run.stepCount}{" "}
                {run.stepCount === 1 ? "step" : "steps"}
              </span>
              {run.finalOutputPreview ? (
                <span className="history-card-snippet">{run.finalOutputPreview}</span>
              ) : null}
            </button>
          </li>
        ))}
      </ul>
      {hasMore ? (
        <button
          type="button"
          className="ghost history-load-more"
          disabled={loadingMore}
          onClick={onLoadMore}
        >
          {loadingMore ? "Loading…" : "Load more"}
        </button>
      ) : null}
    </div>
  );
}

function SearchResults({
  runSteps,
  memories,
  onOpenRun,
}: {
  runSteps: HistorySearchHit[];
  memories: HistorySearchHit[];
  onOpenRun: (runId: string) => void;
}) {
  if (runSteps.length === 0 && memories.length === 0) {
    return <div className="history-message">No history matched this search.</div>;
  }
  return (
    <div className="history-search-groups">
      <SearchGroup
        title="Run steps"
        hits={runSteps}
        onOpen={(hit) => hit.runId && onOpenRun(hit.runId)}
      />
      <SearchGroup
        title="Memories"
        hits={memories}
      />
    </div>
  );
}

function SearchGroup({
  title,
  hits,
  onOpen,
}: {
  title: string;
  hits: HistorySearchHit[];
  onOpen?: (hit: HistorySearchHit) => void;
}) {
  if (hits.length === 0) return null;
  return (
    <section className="history-results" aria-label={title}>
      <div className="history-section-heading">
        <h2>{title}</h2>
        <span>{hits.length}</span>
      </div>
      <ul className="history-list">
        {hits.map((hit) => (
          <li key={`${hit.kind}:${hit.sourceId}`}>
            {onOpen ? (
              <button
                type="button"
                className="history-card"
                onClick={() => onOpen(hit)}
              >
                <SearchHitContent hit={hit} />
              </button>
            ) : (
              <article className="history-card">
                <SearchHitContent hit={hit} />
              </article>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}

function SearchHitContent({ hit }: { hit: HistorySearchHit }) {
  return (
    <>
      <span className="history-card-topline">
        <strong>{hit.title}</strong>
        <span className="history-kind">{historyHitLabel(hit.kind)}</span>
      </span>
      <span className="history-card-meta">
        {hit.workflowName} · {formatHistoryWhen(hit.timestamp)}
      </span>
      <span className="history-card-snippet">
        {literalHistorySnippet(hit.snippet)}
      </span>
    </>
  );
}

function RunDetail({
  detail,
  onBack,
}: {
  detail: RunHistoryDetail;
  onBack: () => void;
}) {
  return (
    <article className="history-detail">
      <button type="button" className="ghost history-detail-back" onClick={onBack}>
        Back to results
      </button>
      <header>
        <div>
          <p className="history-kicker">{detail.run.trigger} run</p>
          <h2>{detail.run.workflowName}</h2>
          <p>{formatHistoryWhen(detail.run.createdAt)}</p>
        </div>
        <span className={`history-status is-${detail.run.status}`}>
          {detail.run.status}
        </span>
      </header>
      {detail.run.error ? (
        <pre className="history-error-text">{detail.run.error}</pre>
      ) : null}
      {detail.steps.length === 0 ? (
        <div className="history-message">This run has no persisted steps.</div>
      ) : (
        <ol className="history-step-list">
          {detail.steps.map((step, index) => (
            <li key={step.id} className="history-step">
              <div className="history-step-heading">
                <div>
                  <span>Step {index + 1}</span>
                  <h3>{step.skillName || step.agentProvider || step.nodeId}</h3>
                  <p>
                    {step.agentProvider || "Local"} · {formatHistoryWhen(step.createdAt)}
                  </p>
                </div>
                <span className={`history-status is-${step.status}`}>{step.status}</span>
              </div>
              {step.error ? <pre className="history-error-text">{step.error}</pre> : null}
              <details>
                <summary>Input</summary>
                <pre>{formatHistoryJson(step.input)}</pre>
              </details>
              <details>
                <summary>Output</summary>
                <pre>{formatHistoryJson(step.output)}</pre>
              </details>
            </li>
          ))}
        </ol>
      )}
    </article>
  );
}
