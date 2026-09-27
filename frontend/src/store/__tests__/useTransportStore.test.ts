/**
 * The transport store: what the listener asked for, and what it tells the attached controller.
 *
 * The store never touches audio itself; the stream hook registers a controller that does. So every action
 * is tested twice over: the state it leaves (which the buttons draw from) and the call the controller
 * receives (which is what actually moves the audio). A fake controller records the calls, and every
 * action is also run with no controller attached, the state before the stream hook mounts.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';

import { useTransportStore } from '../useTransportStore';
import type { TransportController } from '../useTransportStore';

const INITIAL = {
  playing: false,
  mode: 'live' as const,
  followingLive: true,
  requestedPositionMs: null,
  volume: 0.8,
  muted: false,
  speed: 1,
  endOfRecording: false,
  controller: null,
};

function fakeController() {
  return {
    play: vi.fn(async (): Promise<void> => undefined),
    pause: vi.fn(),
    seek: vi.fn(),
    goLive: vi.fn(),
    setVolume: vi.fn(),
    setMuted: vi.fn(),
    setSpeed: vi.fn(),
  } satisfies TransportController;
}

let controller: ReturnType<typeof fakeController>;

beforeEach(() => {
  useTransportStore.setState(INITIAL);
  controller = fakeController();
  useTransportStore.getState().attachController(controller);
});

describe('attachController', () => {
  it('registers the controller and can take it away again', () => {
    expect(useTransportStore.getState().controller).toBe(controller);
    useTransportStore.getState().attachController(null);
    expect(useTransportStore.getState().controller).toBeNull();
  });
});

describe('play and pause', () => {
  it('starts the controller and only then claims to be playing', async () => {
    let finish = () => undefined as void;
    controller.play.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );

    const pending = useTransportStore.getState().play();
    expect(controller.play).toHaveBeenCalledTimes(1);
    expect(useTransportStore.getState().playing).toBe(false);

    finish();
    await pending;
    expect(useTransportStore.getState().playing).toBe(true);
  });

  it('does not claim to be playing when the controller fails to start', async () => {
    controller.play.mockRejectedValue(new Error('NotAllowedError'));
    await expect(useTransportStore.getState().play()).rejects.toThrow('NotAllowedError');
    expect(useTransportStore.getState().playing).toBe(false);
  });

  it('pauses the controller and stops claiming to play', async () => {
    await useTransportStore.getState().play();
    useTransportStore.getState().pause();
    expect(controller.pause).toHaveBeenCalledTimes(1);
    expect(useTransportStore.getState().playing).toBe(false);
  });
});

describe('seek', () => {
  it('moves to playback at the moment asked for, and stops following live', () => {
    useTransportStore.setState({ endOfRecording: true });
    useTransportStore.getState().seek(1_757_000_000_000);

    expect(useTransportStore.getState()).toMatchObject({
      mode: 'playback',
      followingLive: false,
      requestedPositionMs: 1_757_000_000_000,
      endOfRecording: false,
    });
    expect(controller.seek).toHaveBeenCalledWith(1_757_000_000_000);
  });

  it('keeps the speed that was chosen, since history can be played faster', () => {
    useTransportStore.setState({ speed: 2 });
    useTransportStore.getState().seek(1_000);
    expect(useTransportStore.getState().speed).toBe(2);
  });
});

describe('goLive', () => {
  it('returns to the live edge at real time and tells the controller', () => {
    useTransportStore.setState({
      mode: 'playback',
      followingLive: false,
      requestedPositionMs: 5_000,
      endOfRecording: true,
      speed: 2,
    });
    useTransportStore.getState().goLive();

    expect(useTransportStore.getState()).toMatchObject({
      mode: 'live',
      followingLive: true,
      requestedPositionMs: null,
      endOfRecording: false,
      speed: 1,
    });
    expect(controller.goLive).toHaveBeenCalledTimes(1);
  });
});

describe('markLive', () => {
  it('records that the server rejoined the live feed, without telling the controller to', () => {
    useTransportStore.setState({
      mode: 'playback',
      followingLive: false,
      requestedPositionMs: 5_000,
      endOfRecording: true,
      speed: 4,
    });
    useTransportStore.getState().markLive();

    expect(useTransportStore.getState()).toMatchObject({
      mode: 'live',
      followingLive: true,
      requestedPositionMs: null,
      endOfRecording: false,
      speed: 1,
    });
    expect(controller.goLive).not.toHaveBeenCalled();
  });
});

describe('setVolume', () => {
  it('clamps the volume between silent and full and hands the clamped value on', () => {
    useTransportStore.getState().setVolume(1.7);
    expect(useTransportStore.getState().volume).toBe(1);
    expect(controller.setVolume).toHaveBeenLastCalledWith(1);

    useTransportStore.getState().setVolume(-0.3);
    expect(useTransportStore.getState().volume).toBe(0);
    expect(controller.setVolume).toHaveBeenLastCalledWith(0);

    useTransportStore.getState().setVolume(0.35);
    expect(useTransportStore.getState().volume).toBe(0.35);
    expect(controller.setVolume).toHaveBeenLastCalledWith(0.35);
  });

  it('shows the sound as on again when the volume is raised while muted', () => {
    useTransportStore.setState({ muted: true });
    useTransportStore.getState().setVolume(0.5);
    expect(useTransportStore.getState().muted).toBe(false);
  });

  it('unmutes what is heard as well as the button when the volume is raised while muted', () => {
    useTransportStore.setState({ muted: true });
    useTransportStore.getState().setVolume(0.5);
    expect(controller.setMuted).toHaveBeenLastCalledWith(false);
  });

  it('tells the controller nothing about the mute when the sound was already on', () => {
    useTransportStore.getState().setVolume(0.5);
    expect(controller.setMuted).not.toHaveBeenCalled();
  });

  it('leaves the mute alone when the volume is pulled to zero', () => {
    useTransportStore.setState({ muted: true });
    useTransportStore.getState().setVolume(0);
    expect(useTransportStore.getState().muted).toBe(true);

    useTransportStore.setState({ muted: false });
    useTransportStore.getState().setVolume(0);
    expect(useTransportStore.getState().muted).toBe(false);
  });
});

describe('toggleMuted', () => {
  it('flips the mute and tells the controller each time', () => {
    useTransportStore.getState().toggleMuted();
    expect(useTransportStore.getState().muted).toBe(true);
    expect(controller.setMuted).toHaveBeenLastCalledWith(true);

    useTransportStore.getState().toggleMuted();
    expect(useTransportStore.getState().muted).toBe(false);
    expect(controller.setMuted).toHaveBeenLastCalledWith(false);
  });
});

describe('speed', () => {
  it('applies a requested speed at once so the buttons respond, and asks the controller for it', () => {
    useTransportStore.getState().requestSpeed(2);
    expect(useTransportStore.getState().speed).toBe(2);
    expect(controller.setSpeed).toHaveBeenCalledWith(2);
  });

  it('takes the speed the server actually applied without asking again', () => {
    useTransportStore.getState().requestSpeed(8);
    useTransportStore.getState().setAppliedSpeed(4);
    expect(useTransportStore.getState().speed).toBe(4);
    expect(controller.setSpeed).toHaveBeenCalledTimes(1);
  });
});

describe('the plain setters', () => {
  it('record what the stream reports without calling the controller', () => {
    const state = useTransportStore.getState();
    state.setPlaying(true);
    state.setMode('paused');
    state.setEndOfRecording(true);

    expect(useTransportStore.getState()).toMatchObject({
      playing: true,
      mode: 'paused',
      endOfRecording: true,
    });
    for (const call of Object.values(controller)) {
      expect(call).not.toHaveBeenCalled();
    }
  });
});

describe('without a controller', () => {
  beforeEach(() => {
    useTransportStore.getState().attachController(null);
  });

  it('still records every intent, so the buttons work before the stream hook mounts', async () => {
    const state = useTransportStore.getState();
    await state.play();
    expect(useTransportStore.getState().playing).toBe(true);

    state.seek(5_000);
    expect(useTransportStore.getState().requestedPositionMs).toBe(5_000);

    state.setVolume(0.2);
    state.toggleMuted();
    state.requestSpeed(2);
    expect(useTransportStore.getState()).toMatchObject({ volume: 0.2, muted: true, speed: 2 });

    state.goLive();
    expect(useTransportStore.getState().mode).toBe('live');

    state.pause();
    expect(useTransportStore.getState().playing).toBe(false);
  });
});
