// @vitest-environment jsdom

/**
 * Where recordings are written: the path box, its worked examples in the host's own path style, Test,
 * which asks the server what would happen without changing anything, and the way back to the default
 * location. The real settings store holds the draft, and the server's directory check is replaced with a
 * spy, since what the server decides about a folder is tested on the server.
 */

import '@/test/dom';

import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { api, ApiError } from '@/api/client';
import type { DirectoryTest } from '@/api/types';
import { useSettingsStore } from '@/store/useSettingsStore';
import { useStatusStore } from '@/store/useStatusStore';

import { RecordingLocation } from '../RecordingLocation';
import { serviceStatus, STORED } from './fixtures';

const WRITABLE: DirectoryTest = {
  ok: true,
  resolvedPath: '/mnt/audio/on-air',
  exists: true,
  willCreate: false,
  readable: true,
  writable: true,
  message: 'The folder exists and can be written to.',
};

const box = () => screen.getByRole('textbox', { name: 'Recording directory' });
const testButton = () => screen.getByRole('button', { name: /^Test/ });

function renderCard() {
  const user = userEvent.setup();
  render(<RecordingLocation />);
  return user;
}

describe('the recording location card', () => {
  beforeEach(() => {
    useSettingsStore.setState({ settings: STORED, defaults: STORED, draft: {}, error: null });
    useStatusStore.setState({ status: serviceStatus(48_000, 'idle') });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('says it is loading until the settings arrive', () => {
    useSettingsStore.setState({ settings: null });
    render(<RecordingLocation />);
    expect(screen.getByText('Loading...')).toBeInTheDocument();
  });

  describe('showing the location', () => {
    it('leaves the box empty for the default, with the default location as its placeholder', () => {
      render(<RecordingLocation />);
      expect(box()).toHaveValue('');
      expect(box()).toHaveAttribute('placeholder', '/srv/oar/recordings');
      expect(screen.getByText('Currently writing to').nextElementSibling).toHaveTextContent(
        '/srv/oar/recordings',
      );
    });

    it('offers no way back to the default while the default is in use', () => {
      render(<RecordingLocation />);
      expect(screen.queryByRole('button', { name: 'Use the default location' })).not.toBeInTheDocument();
    });

    it('shows a stored folder in the box', () => {
      useSettingsStore.setState({
        settings: { ...STORED, recordingsDir: '/mnt/audio', effectiveRecordingsDir: '/mnt/audio' },
      });
      render(<RecordingLocation />);
      expect(box()).toHaveValue('/mnt/audio');
    });

    it('shows a pending folder over the stored one', () => {
      useSettingsStore.setState({ draft: { recordingsDir: '/mnt/pending' } });
      render(<RecordingLocation />);
      expect(box()).toHaveValue('/mnt/pending');
    });

    it('gives examples with a leading slash when the host uses POSIX paths', () => {
      render(<RecordingLocation />);
      expect(screen.getByText('/mnt/audio/on-air')).toBeInTheDocument();
      expect(screen.getByText(/A path with no leading slash is taken from the data directory/)).toBeInTheDocument();
    });

    it('gives examples with a drive letter when the host runs Windows', () => {
      useSettingsStore.setState({
        settings: { ...STORED, effectiveRecordingsDir: 'C:\\ProgramData\\on-air-record\\recordings' },
      });
      render(<RecordingLocation />);
      expect(screen.getByText('D:\\Recordings\\on-air')).toBeInTheDocument();
      expect(screen.getByText(/A path with no drive letter is taken from the data directory/)).toBeInTheDocument();
    });

    it('says a change waits for the next session while recording', () => {
      useStatusStore.setState({ status: serviceStatus(48_000, 'recording') });
      render(<RecordingLocation />);
      expect(screen.getByText(/takes effect on the next recording session/)).toBeInTheDocument();
      expect(screen.getByText(/Stop and start the recorder to move sooner/)).toBeInTheDocument();
    });

    it('says a change applies when recording next starts while stopped', () => {
      render(<RecordingLocation />);
      expect(screen.getByText(/takes effect the next time recording starts/)).toBeInTheDocument();
      expect(screen.getByText(/Existing recordings stay where they are/)).toBeInTheDocument();
    });
  });

  describe('changing the location', () => {
    it('stages a typed folder in the draft, to wait for Save', async () => {
      const user = renderCard();
      await user.type(box(), '/mnt/audio');
      expect(useSettingsStore.getState().draft).toEqual({ recordingsDir: '/mnt/audio' });
      expect(box()).toHaveValue('/mnt/audio');
    });

    it('stages an emptied box as the default location', async () => {
      useSettingsStore.setState({
        settings: { ...STORED, recordingsDir: '/mnt/audio', effectiveRecordingsDir: '/mnt/audio' },
      });
      const user = renderCard();
      await user.clear(box());
      const { draft } = useSettingsStore.getState();
      expect('recordingsDir' in draft).toBe(true);
      expect(draft.recordingsDir).toBeNull();
    });

    it('stages nothing for a box of only spaces while the default is in use', async () => {
      const user = renderCard();
      await user.type(box(), '   ');
      expect(useSettingsStore.getState().draft).toEqual({});
    });

    it('stages the default location from its button, which then goes away', async () => {
      useSettingsStore.setState({
        settings: { ...STORED, recordingsDir: '/mnt/audio', effectiveRecordingsDir: '/mnt/audio' },
      });
      const user = renderCard();
      await user.click(screen.getByRole('button', { name: 'Use the default location' }));

      expect(useSettingsStore.getState().draft.recordingsDir).toBeNull();
      expect(box()).toHaveValue('');
      expect(screen.queryByRole('button', { name: 'Use the default location' })).not.toBeInTheDocument();
    });

    it('keeps the spaces in a folder name as it is typed', async () => {
      const user = renderCard();
      await user.type(box(), '/Volumes/My Drive/Program Files');
      expect(box()).toHaveValue('/Volumes/My Drive/Program Files');
      expect(useSettingsStore.getState().draft).toEqual({
        recordingsDir: '/Volumes/My Drive/Program Files',
      });
    });

    it('offers the way back to the default as soon as a folder is typed', async () => {
      const user = renderCard();
      await user.type(box(), '/mnt');
      expect(screen.getByRole('button', { name: 'Use the default location' })).toBeInTheDocument();
    });
  });

  describe('testing a folder', () => {
    it('tests the folder without the spaces around it', async () => {
      const probe = vi.spyOn(api, 'testRecordingsDir').mockResolvedValue(WRITABLE);
      useSettingsStore.setState({ draft: { recordingsDir: '  /Volumes/My Drive  ' } });
      const user = renderCard();
      await user.click(testButton());
      expect(probe).toHaveBeenCalledWith('/Volumes/My Drive');
    });

    it('asks the server about the folder in the box and shows its verdict', async () => {
      const probe = vi.spyOn(api, 'testRecordingsDir').mockResolvedValue(WRITABLE);
      useSettingsStore.setState({ draft: { recordingsDir: '/mnt/audio/on-air' } });
      const user = renderCard();
      await user.click(testButton());

      expect(probe).toHaveBeenCalledWith('/mnt/audio/on-air');
      expect(screen.getByText('The folder exists and can be written to.')).toBeInTheDocument();
      expect(screen.getAllByText('/mnt/audio/on-air').length).toBeGreaterThan(0);
    });

    it('asks about the default location when the box is empty', async () => {
      const probe = vi
        .spyOn(api, 'testRecordingsDir')
        .mockResolvedValue({ ...WRITABLE, resolvedPath: '/srv/oar/recordings' });
      const user = renderCard();
      await user.click(testButton());
      expect(probe).toHaveBeenCalledWith(null);
    });

    it('tests when Enter is pressed in the box', async () => {
      const probe = vi.spyOn(api, 'testRecordingsDir').mockResolvedValue(WRITABLE);
      const user = renderCard();
      await user.type(box(), '/mnt/x{Enter}');
      expect(probe).toHaveBeenCalledWith('/mnt/x');
    });

    it('changes nothing in the draft by testing', async () => {
      vi.spyOn(api, 'testRecordingsDir').mockResolvedValue(WRITABLE);
      const user = renderCard();
      await user.click(testButton());
      expect(useSettingsStore.getState().draft).toEqual({});
    });

    it('shows a folder the server refuses, with its reason', async () => {
      vi.spyOn(api, 'testRecordingsDir').mockResolvedValue({
        ok: false,
        resolvedPath: '/root/locked',
        exists: true,
        willCreate: false,
        readable: true,
        writable: false,
        message: 'The folder exists but cannot be written to.',
      });
      useSettingsStore.setState({ draft: { recordingsDir: '/root/locked' } });
      const user = renderCard();
      await user.click(testButton());
      expect(screen.getByText('The folder exists but cannot be written to.')).toBeInTheDocument();
    });

    it('says what went wrong when the server cannot be asked', async () => {
      vi.spyOn(api, 'testRecordingsDir').mockRejectedValue(
        new ApiError('the service is restarting', 'unavailable', 503),
      );
      useSettingsStore.setState({ draft: { recordingsDir: '/mnt/audio' } });
      const user = renderCard();
      await user.click(testButton());
      expect(screen.getByText('the service is restarting')).toBeInTheDocument();
      expect(testButton()).toBeEnabled();
    });

    it('says Testing and refuses a second press while the server is answering', async () => {
      let answer: (result: DirectoryTest) => void = () => undefined;
      const probe = vi
        .spyOn(api, 'testRecordingsDir')
        .mockImplementation(() => new Promise<DirectoryTest>((resolve) => (answer = resolve)));
      const user = renderCard();
      await user.click(testButton());

      expect(testButton()).toHaveTextContent('Testing');
      expect(testButton()).toBeDisabled();
      await user.click(testButton());
      expect(probe).toHaveBeenCalledTimes(1);

      await act(async () => {
        answer(WRITABLE);
      });
      expect(testButton()).toHaveTextContent('Test');
      expect(testButton()).toBeEnabled();
    });

    it('drops the verdict once the folder is edited, since it no longer applies', async () => {
      vi.spyOn(api, 'testRecordingsDir').mockResolvedValue(WRITABLE);
      useSettingsStore.setState({ draft: { recordingsDir: '/mnt/audio/on-air' } });
      const user = renderCard();
      await user.click(testButton());
      expect(screen.getByText(WRITABLE.message)).toBeInTheDocument();

      await user.type(box(), '2');
      expect(screen.queryByText(WRITABLE.message)).not.toBeInTheDocument();
    });

    it('brings the verdict back when the folder is edited back to the one tested', async () => {
      vi.spyOn(api, 'testRecordingsDir').mockResolvedValue(WRITABLE);
      useSettingsStore.setState({ draft: { recordingsDir: '/mnt/audio/on-air' } });
      const user = renderCard();
      await user.click(testButton());
      await user.type(box(), '2{Backspace}');
      expect(screen.getByText(WRITABLE.message)).toBeInTheDocument();
    });
  });
});
