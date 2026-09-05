/**
 * The one place settings are committed.
 *
 * Sticks to the bottom of the page so the Save button is reachable without scrolling back, and stays out
 * of the way entirely until there is something to save.
 *
 * Shrinking the retention window is the only genuinely destructive change here, so that is the only one
 * that asks twice. Everything else saves on the first click.
 */

import { RotateCcw, Save, TriangleAlert, Undo2 } from 'lucide-react';
import { useState } from 'react';

import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import type { Settings } from '@/api/types';
import { useSettingsStore } from '@/store/useSettingsStore';

/** Describe a retention window the way the panel above does. */
function windowLabel(hours: number | null | undefined): string {
  if (hours === null || hours === undefined) {
    return 'forever';
  }
  if (hours > 24 && hours % 24 === 0) {
    return `${hours / 24} days`;
  }
  return `${hours} hours`;
}

/**
 * True when saving would make the janitor delete audio that currently exists.
 *
 * Coming back from "keep forever" always qualifies, however long the new window is.
 */
export function shortensRetention(stored: Settings | null, pending: Settings | null): boolean {
  if (!stored || !pending || pending.retentionHours === null) {
    return false;
  }
  return stored.retentionHours === null || pending.retentionHours < stored.retentionHours;
}

export function SettingsActionBar() {
  const settings = useSettingsStore((state) => state.settings);
  const defaults = useSettingsStore((state) => state.defaults);
  const draft = useSettingsStore((state) => state.draft);
  const saving = useSettingsStore((state) => state.saving);
  const error = useSettingsStore((state) => state.error);
  const stageDefaults = useSettingsStore((state) => state.stageDefaults);
  const discard = useSettingsStore((state) => state.discard);
  const save = useSettingsStore((state) => state.save);

  const [confirming, setConfirming] = useState(false);

  const changeCount = Object.keys(draft).length;
  const dirty = changeCount > 0;
  const pending = settings ? { ...settings, ...draft } : null;
  const destructive = shortensRetention(settings, pending);

  const atDefaults =
    defaults !== null &&
    pending !== null &&
    (
      [
        'gain',
        'segmentSeconds',
        'retentionHours',
        'autoStart',
        'recordingSampleRate',
        'recordingsDir',
      ] as const
    ).every(
      (key) => Object.is(pending[key], defaults[key]),
    );

  return (
    <div className="bg-background/90 sticky bottom-0 z-10 -mx-4 border-t px-4 py-3 backdrop-blur">
      <div className="mx-auto flex max-w-3xl flex-wrap items-center gap-3">
        <div className="min-w-0">
          {dirty ? (
            <Badge variant="secondary" className="font-normal">
              {changeCount} unsaved {changeCount === 1 ? 'change' : 'changes'}
            </Badge>
          ) : (
            <span className="text-muted-foreground text-xs">
              {atDefaults ? 'Everything is at its default.' : 'All changes saved.'}
            </span>
          )}
        </div>

        <div className="ml-auto flex items-center gap-2">
          <Button
            size="sm"
            variant="ghost"
            disabled={saving || atDefaults}
            onClick={stageDefaults}
          >
            <RotateCcw />
            Restore defaults
          </Button>

          <Button size="sm" variant="outline" disabled={!dirty || saving} onClick={discard}>
            <Undo2 />
            Discard
          </Button>

          {destructive ? (
            <Popover open={confirming} onOpenChange={setConfirming}>
              <PopoverTrigger asChild>
                <Button size="sm" disabled={!dirty || saving}>
                  <Save />
                  Save changes
                </Button>
              </PopoverTrigger>
              <PopoverContent side="top" align="end" className="w-72 space-y-3 p-3">
                <p className="text-sm font-medium">Save and delete older audio?</p>
                <p className="text-destructive flex items-start gap-1.5 text-xs">
                  <TriangleAlert className="mt-0.5 size-3.5 shrink-0" />
                  Retention goes from {windowLabel(settings?.retentionHours)} to{' '}
                  {windowLabel(pending?.retentionHours)}. Anything older is deleted within a minute and
                  cannot be recovered.
                </p>
                <div className="flex justify-end gap-2 pt-1">
                  <Button size="sm" variant="ghost" onClick={() => setConfirming(false)}>
                    Cancel
                  </Button>
                  <Button
                    size="sm"
                    variant="destructive"
                    disabled={saving}
                    onClick={() => {
                      void save();
                      setConfirming(false);
                    }}
                  >
                    Save and delete
                  </Button>
                </div>
              </PopoverContent>
            </Popover>
          ) : (
            <Button size="sm" disabled={!dirty || saving} onClick={() => void save()}>
              <Save />
              {saving ? 'Saving...' : 'Save changes'}
            </Button>
          )}
        </div>

        {error && <p className="text-destructive w-full text-xs">{error}</p>}
      </div>
    </div>
  );
}
