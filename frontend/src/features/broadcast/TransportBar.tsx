/**
 * Playback transport: the controls a listener actually touches.
 *
 * The offset readout is the important part. On a DVR, "where am I" is not a position in a file, it is a
 * distance from now, so it is expressed that way rather than as an absolute timestamp alone.
 */

import { Pause, Play, Radio, RotateCcw, Volume2, VolumeX } from 'lucide-react';
import { useState } from 'react';

import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Slider } from '@/components/ui/slider';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { useAnimationFrame } from '@/hooks/useAnimationFrame';
import { formatClock, formatOffsetFromLive } from '@/lib/format';
import { useConnectionStore } from '@/store/useConnectionStore';
import { useTimelineStore } from '@/store/useTimelineStore';
import { useTransportStore } from '@/store/useTransportStore';

type TransportBarProps = {
  getPlayheadMs: () => number | null;
};

/** How far behind the live edge before the UI stops calling it live. */
const LIVE_TOLERANCE_MS = 2500;

export function TransportBar({ getPlayheadMs }: TransportBarProps) {
  const playing = useTransportStore((state) => state.playing);
  const mode = useTransportStore((state) => state.mode);
  const volume = useTransportStore((state) => state.volume);
  const muted = useTransportStore((state) => state.muted);
  const endOfRecording = useTransportStore((state) => state.endOfRecording);
  const requestedPositionMs = useTransportStore((state) => state.requestedPositionMs);
  const play = useTransportStore((state) => state.play);
  const pause = useTransportStore((state) => state.pause);
  const goLive = useTransportStore((state) => state.goLive);
  const seek = useTransportStore((state) => state.seek);
  const setVolume = useTransportStore((state) => state.setVolume);
  const toggleMuted = useTransportStore((state) => state.toggleMuted);

  const connected = useConnectionStore((state) => state.connected);
  const liveEdgeMs = useTimelineStore((state) => state.range?.liveEdgeMs ?? null);

  // The playhead moves every frame. Holding it in state at 60 Hz would re render this whole bar, so it is
  // sampled on an animation frame and only committed when the displayed value would actually change.
  const [playheadLabel, setPlayheadLabel] = useState('--:--:--');
  const [offsetLabel, setOffsetLabel] = useState('live');
  const [behind, setBehind] = useState(false);

  useAnimationFrame(() => {
    // Same fallback as the timeline marker: show the moment that was asked for until the audio clock has
    // something better, so the readout and the marker always agree.
    const playhead = getPlayheadMs() ?? requestedPositionMs;
    const nextClock = formatClock(playhead);
    if (nextClock !== playheadLabel) {
      setPlayheadLabel(nextClock);
    }

    if (playhead === null || liveEdgeMs === null) {
      if (offsetLabel !== 'live') {
        setOffsetLabel('live');
      }
      if (behind) {
        setBehind(false);
      }
      return;
    }

    const offset = Math.max(liveEdgeMs - playhead, 0);
    const nextOffset = formatOffsetFromLive(offset);
    if (nextOffset !== offsetLabel) {
      setOffsetLabel(nextOffset);
    }
    const nextBehind = offset > LIVE_TOLERANCE_MS;
    if (nextBehind !== behind) {
      setBehind(nextBehind);
    }
  });

  const rewind = (seconds: number) => {
    const anchor = getPlayheadMs() ?? liveEdgeMs ?? Date.now();
    seek(anchor - seconds * 1000);
  };

  return (
    <div className="flex flex-wrap items-center gap-3">
      <Button
        size="icon"
        variant={playing ? 'secondary' : 'default'}
        onClick={() => (playing ? pause() : void play())}
        aria-label={playing ? 'Pause' : 'Play'}
        disabled={!connected}
      >
        {playing ? <Pause /> : <Play />}
      </Button>

      <Tooltip>
        <TooltipTrigger asChild>
          <Button size="icon" variant="outline" onClick={() => rewind(30)} aria-label="Back 30 seconds">
            <RotateCcw />
          </Button>
        </TooltipTrigger>
        <TooltipContent>Jump back 30 seconds</TooltipContent>
      </Tooltip>

      <Button
        variant={behind || mode !== 'live' ? 'default' : 'secondary'}
        onClick={goLive}
        disabled={!connected}
      >
        <Radio />
        Go live
      </Button>

      <div className="flex items-baseline gap-2">
        <span className="text-lg font-semibold tabular">{playheadLabel}</span>
        {!playing ? (
          <Badge variant="outline">{requestedPositionMs === null ? 'not playing' : 'cued'}</Badge>
        ) : behind || mode === 'playback' ? (
          <Badge variant="secondary">{offsetLabel}</Badge>
        ) : (
          <Badge variant="live">on air</Badge>
        )}
        {endOfRecording && <Badge variant="outline">end of recording</Badge>}
      </div>

      <div className="ml-auto flex w-44 items-center gap-2">
        <Button
          size="icon-sm"
          variant="ghost"
          onClick={toggleMuted}
          aria-label={muted ? 'Unmute' : 'Mute'}
        >
          {muted || volume === 0 ? <VolumeX /> : <Volume2 />}
        </Button>
        <Slider
          value={[muted ? 0 : volume]}
          min={0}
          max={1}
          step={0.01}
          onValueChange={([next]) => setVolume(next ?? 0)}
          aria-label="Volume"
        />
      </div>
    </div>
  );
}
