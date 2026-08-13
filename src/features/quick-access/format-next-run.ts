export function formatQuickAccessNextRun(
  value: string | null | undefined,
  now = Date.now(),
  timeZone?: string,
): string {
  if (!value) return "Not scheduled";
  const next = new Date(value);
  if (Number.isNaN(next.getTime())) return value;

  const deltaMinutes = Math.ceil((next.getTime() - now) / 60_000);
  if (deltaMinutes <= 0) return "Due now";
  if (deltaMinutes < 60) return `In ${deltaMinutes} min`;

  const sameDay = new Intl.DateTimeFormat("en-CA", {
    timeZone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  });
  const nextDay = sameDay.format(next);
  const currentDay = sameDay.format(new Date(now));
  const time = next.toLocaleTimeString(undefined, {
    timeZone,
    hour: "2-digit",
    minute: "2-digit",
  });
  if (nextDay === currentDay) return `Today at ${time}`;

  return next.toLocaleString(undefined, {
    timeZone,
    weekday: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
}
