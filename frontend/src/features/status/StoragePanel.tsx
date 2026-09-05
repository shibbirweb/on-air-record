/** Disk usage and how far back the DVR reaches. */

import { HardDrive } from 'lucide-react';

import { Separator } from '@/components/ui/separator';
import { formatBytes, formatDateTime, formatDuration } from '@/lib/format';
import { useStorageStore } from '@/store/useStorageStore';

export function StoragePanel() {
  const storage = useStorageStore((state) => state.storage);
  const sessions = useStorageStore((state) => state.sessions);

  const historyMs =
    storage?.oldestMs && storage?.newestMs ? storage.newestMs - storage.oldestMs : 0;

  return (
    <div className="space-y-3 text-sm">
      <dl className="grid grid-cols-2 gap-x-4 gap-y-2">
        <dt className="text-muted-foreground">On disk</dt>
        <dd className="tabular">{formatBytes(storage?.bytes ?? 0)}</dd>

        <dt className="text-muted-foreground">Segments</dt>
        <dd className="tabular">{storage?.segmentCount ?? 0}</dd>

        <dt className="text-muted-foreground">History</dt>
        <dd className="tabular">{formatDuration(historyMs)}</dd>

        <dt className="text-muted-foreground">Retention</dt>
        <dd className="tabular">{storage?.retentionHours ?? 0} h</dd>

        <dt className="text-muted-foreground">Oldest</dt>
        <dd className="tabular">{formatDateTime(storage?.oldestMs)}</dd>
      </dl>

      <Separator />

      <div className="space-y-2">
        <p className="text-muted-foreground flex items-center gap-1.5 text-xs">
          <HardDrive className="size-3.5" />
          Recent sessions
        </p>
        {sessions.length === 0 ? (
          <p className="text-muted-foreground text-xs">No sessions recorded yet.</p>
        ) : (
          <ul className="space-y-1.5">
            {sessions.slice(0, 5).map((session) => (
              <li key={session.id} className="flex items-baseline justify-between gap-2 text-xs">
                <span className="truncate">{session.deviceName}</span>
                <span className="text-muted-foreground shrink-0 tabular">
                  {formatDateTime(session.startedAtMs)} · {formatBytes(session.bytes)}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>

      {storage?.dataDir && (
        <p className="text-muted-foreground truncate font-mono text-[10px]" title={storage.dataDir}>
          {storage.dataDir}
        </p>
      )}
    </div>
  );
}
