import { useCallback, useEffect, useState } from "react";
import * as api from "../../api";
import { SCHEDULE_PRESETS, type ScheduleListItem } from "../../types";

type Props = {
  onClose: () => void;
  onOpenWorkflow: (workflowId: string) => void;
  onEditSchedule: (workflowId: string, workflowName: string) => void;
};

function formatNextRun(value: string | null | undefined) {
  if (!value) return "Not scheduled";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

function cronLabel(cron: string) {
  const preset = SCHEDULE_PRESETS.find((p) => p.cron === cron);
  return preset?.label ?? cron;
}

export function SchedulesPage({
  onClose,
  onOpenWorkflow,
  onEditSchedule,
}: Props) {
  const [rows, setRows] = useState<ScheduleListItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const next = await api.listSchedules();
      setRows(next);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const enabledCount = rows.filter((r) => r.enabled).length;

  return (
    <section className="settings-page schedules-page" aria-label="Schedules">
      <header className="settings-page-header">
        <div>
          <p className="settings-kicker">Automations</p>
          <h1>Schedules</h1>
          <p className="schedules-page-lead">
            Cron-based runs across your workflows
            {rows.length > 0
              ? ` · ${enabledCount} active of ${rows.length}`
              : ""}
            .
          </p>
        </div>
        <button type="button" className="ghost" onClick={onClose}>
          Back to canvas
        </button>
      </header>

      <div className="settings-page-body">
        {error ? <p className="error">{error}</p> : null}

        {loading ? (
          <p className="muted">Loading schedules…</p>
        ) : rows.length === 0 ? (
          <div className="settings-card schedules-empty">
            <p className="settings-label">No schedules yet</p>
            <p className="settings-value">
              Open a workflow, right-click it in the sidebar, and choose
              Schedule… to run it on a cadence.
            </p>
          </div>
        ) : (
          <ul className="schedules-list">
            {rows.map((row) => (
              <li key={row.id} className="schedules-card">
                <div className="schedules-card-main">
                  <div className="schedules-card-top">
                    <button
                      type="button"
                      className="schedules-card-name"
                      onClick={() => onOpenWorkflow(row.workflowId)}
                    >
                      {row.workflowName}
                    </button>
                    <span
                      className={`schedules-status${
                        row.enabled ? " is-on" : ""
                      }`}
                    >
                      {row.enabled ? "On" : "Off"}
                    </span>
                  </div>
                  <p className="schedules-card-meta">
                    {cronLabel(row.cron)}
                    <span aria-hidden> · </span>
                    Next {formatNextRun(row.nextRunAt)}
                  </p>
                </div>
                <div className="schedules-card-actions">
                  <button
                    type="button"
                    className="ghost"
                    onClick={() =>
                      onEditSchedule(row.workflowId, row.workflowName)
                    }
                  >
                    Edit
                  </button>
                  <button
                    type="button"
                    className="ghost"
                    onClick={() => onOpenWorkflow(row.workflowId)}
                  >
                    Open
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}

        <button
          type="button"
          className="ghost schedules-refresh"
          onClick={() => void refresh()}
          disabled={loading}
        >
          Refresh
        </button>
      </div>
    </section>
  );
}
