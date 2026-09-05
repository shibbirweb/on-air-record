/**
 * Calendar day helpers, the browser side counterpart of `backend/src/util/day.rs`.
 *
 * A day id is the `YYYY-MM-DD` string the backend files recordings under. Everything here works in the
 * browser's local timezone, which is the same one the recorder used in the normal case of a machine on
 * the same network.
 */

/** Local `YYYY-MM-DD` for an instant, matching how the backend names a day. */
export function localDayId(timestampMs: number): string {
  const date = new Date(timestampMs);
  const month = `${date.getMonth() + 1}`.padStart(2, '0');
  const day = `${date.getDate()}`.padStart(2, '0');
  return `${date.getFullYear()}-${month}-${day}`;
}

/**
 * The calendar day before `timestampMs`.
 *
 * Stepped by date rather than by subtracting 24 hours, so it stays correct on a daylight saving boundary
 * where a local day is 23 or 25 hours long.
 */
export function previousDayId(timestampMs: number): string {
  const date = new Date(timestampMs);
  date.setDate(date.getDate() - 1);
  return localDayId(date.getTime());
}

/**
 * Turn a day id into local midnight.
 *
 * Built from the parts rather than through `new Date('2026-09-05')`, which the language parses as UTC and
 * would therefore land on the previous day for anyone west of Greenwich.
 */
export function parseDayId(day: string): Date | null {
  const parts = day.split('-');
  if (parts.length !== 3) {
    return null;
  }

  const [year, month, date] = parts.map(Number);
  if (!Number.isInteger(year) || !Number.isInteger(month) || !Number.isInteger(date)) {
    return null;
  }
  if (month < 1 || month > 12 || date < 1 || date > 31) {
    return null;
  }

  const parsed = new Date(year, month - 1, date);
  // Rejects the likes of 2026-02-30, which the Date constructor would silently roll into March.
  if (parsed.getFullYear() !== year || parsed.getMonth() !== month - 1 || parsed.getDate() !== date) {
    return null;
  }

  return parsed;
}

/**
 * Human label for a day, with today and yesterday called out by name.
 *
 * `todayId` is passed in rather than read from the clock so the caller can source it from the server,
 * whose timezone is the one the days were named in, and so this stays a pure function.
 */
export function dayLabel(
  day: string,
  todayId: string | null,
  yesterdayId: string | null,
): string {
  if (todayId !== null && day === todayId) {
    return 'Today';
  }
  if (yesterdayId !== null && day === yesterdayId) {
    return 'Yesterday';
  }

  const parsed = parseDayId(day);
  if (!parsed) {
    return day;
  }

  return parsed.toLocaleDateString(undefined, {
    weekday: 'short',
    month: 'short',
    day: 'numeric',
  });
}
