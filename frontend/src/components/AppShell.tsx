/**
 * Chrome shared by every page.
 *
 * The shell, not the page, owns the audio pipeline. Navigating to the settings page must not silence the
 * broadcast or drop the WebSocket, and it would do both if the engine lived inside the control room and
 * unmounted with it. Pages reach the engine through the router outlet context.
 */

import { Radio, Settings2, SlidersHorizontal, Wifi, WifiOff } from 'lucide-react';
import { NavLink, Outlet, useOutletContext } from 'react-router';

import { AppFooter } from '@/components/AppFooter';
import { OnAirSign } from '@/components/OnAirSign';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { usePolling } from '@/hooks/usePolling';
import { useStreamEngine } from '@/hooks/useStreamEngine';
import { useTheme } from '@/hooks/useTheme';
import type { AudioEngine } from '@/lib/audio/audioEngine';
import { cn } from '@/lib/utils';
import { useConnectionStore } from '@/store/useConnectionStore';
import { useStatusStore } from '@/store/useStatusStore';
import { useTransportStore } from '@/store/useTransportStore';
import { Moon, Sun } from 'lucide-react';

const STATUS_POLL_MS = 1000;

export type AppOutletContext = {
  engine: AudioEngine;
  /** Reads the playhead straight off the audio clock. */
  playheadMs: () => number | null;
};

/** Typed access to what the shell hands its pages. */
export function useAppContext(): AppOutletContext {
  return useOutletContext<AppOutletContext>();
}

const NAV = [
  { to: '/', label: 'Control room', icon: SlidersHorizontal },
  { to: '/settings', label: 'Settings', icon: Settings2 },
] as const;

export function AppShell() {
  const { engine, playheadMs } = useStreamEngine();
  const { theme, toggleTheme } = useTheme();

  const refreshStatus = useStatusStore((state) => state.refresh);
  usePolling(refreshStatus, STATUS_POLL_MS);

  const connected = useConnectionStore((state) => state.connected);
  const capturing = useStatusStore((state) => state.status?.capture.state === 'recording');
  const listeners = useStatusStore((state) => state.status?.listeners ?? 0);
  const playing = useTransportStore((state) => state.playing);
  const mode = useTransportStore((state) => state.mode);

  const onAir = capturing && playing && mode === 'live';

  return (
    <div className="bg-background flex min-h-screen flex-col">
      <header className="bg-background/85 sticky top-0 z-20 border-b backdrop-blur">
        <div className="mx-auto flex max-w-[1600px] flex-wrap items-center gap-3 px-4 py-3">
          <div className="flex items-center gap-2.5">
            <div className="bg-primary text-primary-foreground grid size-9 place-items-center rounded-lg">
              <Radio className="size-5" />
            </div>
            <div className="leading-tight">
              <h1 className="text-base font-semibold">On Air Record</h1>
              <p className="text-muted-foreground text-xs">Local network audio broadcast and DVR</p>
            </div>
          </div>

          <nav className="bg-muted ml-2 flex items-center gap-1 rounded-lg p-1">
            {NAV.map((item) => (
              <NavLink
                key={item.to}
                to={item.to}
                end={item.to === '/'}
                className={({ isActive }) =>
                  cn(
                    'flex items-center gap-1.5 rounded-md px-2.5 py-1 text-sm font-medium transition-colors',
                    isActive
                      ? 'bg-background text-foreground shadow-sm'
                      : 'text-muted-foreground hover:text-foreground',
                  )
                }
              >
                <item.icon className="size-3.5" />
                {item.label}
              </NavLink>
            ))}
          </nav>

          <OnAirSign live={onAir} />

          <div className="ml-auto flex items-center gap-2">
            <Badge variant={connected ? 'secondary' : 'destructive'} className="gap-1.5 font-normal">
              {connected ? <Wifi className="size-3" /> : <WifiOff className="size-3" />}
              {connected ? 'Stream connected' : 'Stream offline'}
            </Badge>
            <Badge variant="outline" className="font-normal">
              {listeners} {listeners === 1 ? 'listener' : 'listeners'}
            </Badge>
            <Button
              size="icon-sm"
              variant="ghost"
              onClick={toggleTheme}
              aria-label="Toggle colour theme"
            >
              {theme === 'dark' ? <Sun /> : <Moon />}
            </Button>
          </div>
        </div>
      </header>

      {/* The shell is a column and the page grows, so the footer sits at the bottom of a short page
          rather than floating half way up it. */}
      <div className="flex-1">
        <Outlet context={{ engine, playheadMs } satisfies AppOutletContext} />
      </div>

      <AppFooter />
    </div>
  );
}
