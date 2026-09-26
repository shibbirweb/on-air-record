/**
 * Adding and revisiting named moments.
 *
 * A bookmark is placed at whatever is playing, not at the middle of the view, because the moment worth
 * marking is the one you just heard. When nothing is playing it falls back to the live edge, which is the
 * only other position the operator can be said to be at.
 */

import { Bookmark as BookmarkIcon, BookmarkPlus, Check, Trash2, X } from 'lucide-react';
import { useState } from 'react';

import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Separator } from '@/components/ui/separator';
import { formatClock, formatDateTime } from '@/lib/format';
import { useCanAdminister } from '@/store/useAuthStore';
import { useBookmarkStore } from '@/store/useBookmarkStore';
import { useTimelineStore } from '@/store/useTimelineStore';
import { useTransportStore } from '@/store/useTransportStore';

type BookmarkControlsProps = {
  getPlayheadMs: () => number | null;
};

export function BookmarkControls({ getPlayheadMs }: BookmarkControlsProps) {
  const bookmarks = useBookmarkStore((state) => state.bookmarks);
  const saving = useBookmarkStore((state) => state.saving);
  const error = useBookmarkStore((state) => state.error);
  const add = useBookmarkStore((state) => state.add);
  const remove = useBookmarkStore((state) => state.remove);
  const clearError = useBookmarkStore((state) => state.clearError);
  // Listeners can jump to bookmarks but not add or remove them.
  const mayAdminister = useCanAdminister();

  const requestedPositionMs = useTransportStore((state) => state.requestedPositionMs);
  const seek = useTransportStore((state) => state.seek);
  const focusOn = useTimelineStore((state) => state.focusOn);
  const liveEdgeMs = useTimelineStore((state) => state.range?.liveEdgeMs ?? null);

  const [adding, setAdding] = useState(false);
  const [label, setLabel] = useState('');
  /** The moment captured when the form opened, so a live playhead does not drift while you type. */
  const [pendingAt, setPendingAt] = useState<number | null>(null);
  const [listOpen, setListOpen] = useState(false);

  const openForm = () => {
    setPendingAt(getPlayheadMs() ?? requestedPositionMs ?? liveEdgeMs ?? Date.now());
    setLabel('');
    clearError();
    setAdding(true);
  };

  const submit = async () => {
    if (pendingAt === null || label.trim() === '') {
      return;
    }

    const created = await add(pendingAt, label);
    if (created) {
      setAdding(false);
      setLabel('');
    }
  };

  const jumpTo = (timestampMs: number) => {
    seek(timestampMs);
    focusOn(timestampMs);
    setListOpen(false);
  };

  return (
    <div className="flex items-center gap-1">
      {mayAdminister && (
        <Popover open={adding} onOpenChange={(open) => (open ? openForm() : setAdding(false))}>
          <PopoverTrigger asChild>
            <Button size="icon-sm" variant="outline" aria-label="Add a bookmark here">
              <BookmarkPlus />
            </Button>
          </PopoverTrigger>

          <PopoverContent align="start" className="w-72 space-y-3 p-3">
            <div className="space-y-1">
              <p className="text-sm font-medium">Bookmark this moment</p>
              <p className="text-muted-foreground text-xs tabular">
                {pendingAt === null ? '' : formatDateTime(pendingAt)}
              </p>
            </div>

            <Input
              autoFocus
              value={label}
              maxLength={120}
              placeholder="What happened here?"
              onChange={(event) => setLabel(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  void submit();
                }
                if (event.key === 'Escape') {
                  setAdding(false);
                }
              }}
            />

            {error && <p className="text-destructive text-xs">{error}</p>}

            <div className="flex justify-end gap-2">
              <Button size="sm" variant="ghost" onClick={() => setAdding(false)}>
                <X />
                Cancel
              </Button>
              <Button size="sm" disabled={saving || label.trim() === ''} onClick={() => void submit()}>
                <Check />
                Save
              </Button>
            </div>
          </PopoverContent>
        </Popover>
      )}

      <Popover open={listOpen} onOpenChange={setListOpen}>
        <PopoverTrigger asChild>
          <Button size="sm" variant="ghost" className="h-8 gap-1.5 px-2">
            <BookmarkIcon className="size-3.5" />
            {bookmarks.length}
          </Button>
        </PopoverTrigger>

        <PopoverContent align="start" className="w-80 p-0">
          <p className="px-3 py-2 text-sm font-medium">
            {bookmarks.length === 0
              ? 'No bookmarks yet'
              : `${bookmarks.length} ${bookmarks.length === 1 ? 'bookmark' : 'bookmarks'}`}
          </p>
          <Separator />

          {bookmarks.length === 0 ? (
            <p className="text-muted-foreground px-3 py-3 text-xs">
              Mark a moment while listening and it appears here, on the timeline, and on the day
              overview.
            </p>
          ) : (
            <ul className="max-h-72 overflow-y-auto py-1">
              {/* Newest first here, the reverse of the timeline, because the thing just marked is the
                  one most likely to be wanted back. */}
              {[...bookmarks].reverse().map((bookmark) => (
                <li key={bookmark.id} className="group flex items-center gap-2 px-2 py-1">
                  <button
                    type="button"
                    className="hover:bg-accent min-w-0 flex-1 rounded-md px-2 py-1 text-left"
                    onClick={() => jumpTo(bookmark.timestampMs)}
                  >
                    <span className="block truncate text-sm">{bookmark.label}</span>
                    <span className="text-muted-foreground block text-[11px] tabular">
                      {formatDateTime(bookmark.timestampMs)} at {formatClock(bookmark.timestampMs)}
                    </span>
                  </button>
                  {mayAdminister && (
                    <Button
                      size="icon-sm"
                      variant="ghost"
                      disabled={saving}
                      aria-label={`Remove ${bookmark.label}`}
                      onClick={() => void remove(bookmark.id)}
                    >
                      <Trash2 />
                    </Button>
                  )}
                </li>
              ))}
            </ul>
          )}
        </PopoverContent>
      </Popover>
    </div>
  );
}
