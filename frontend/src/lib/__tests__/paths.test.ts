import { describe, expect, it } from 'vitest';

import { pathExamples, pathStyleOf } from '../paths';

describe('pathStyleOf', () => {
  it('recognises a drive letter', () => {
    expect(pathStyleOf('C:\\Users\\shibbir\\data')).toBe('windows');
    expect(pathStyleOf('D:/recordings')).toBe('windows');
  });

  it('recognises a UNC share', () => {
    expect(pathStyleOf('\\\\server\\share\\audio')).toBe('windows');
  });

  it('treats everything else as posix', () => {
    expect(pathStyleOf('/srv/oar/recordings')).toBe('posix');
    expect(pathStyleOf('/Users/shibbir/data')).toBe('posix');
    expect(pathStyleOf('')).toBe('posix');
  });
});

describe('pathExamples', () => {
  it('offers examples in the server platform convention, not the viewer one', () => {
    // A browser on macOS configuring a Windows server must still be shown Windows examples.
    const windows = pathExamples('C:\\ProgramData\\on-air-record\\recordings');
    expect(windows.style).toBe('windows');
    expect(windows.absolute).toMatch(/^[A-Za-z]:\\/);

    const posix = pathExamples('/srv/oar/recordings');
    expect(posix.style).toBe('posix');
    expect(posix.absolute.startsWith('/')).toBe(true);
  });

  it('always offers a relative example that is not anchored to a root', () => {
    for (const sample of ['/srv/oar/recordings', 'C:\\oar\\recordings']) {
      const examples = pathExamples(sample);
      expect(examples.relative.startsWith('/')).toBe(false);
      expect(examples.relative).not.toMatch(/^[A-Za-z]:/);
    }
  });
});
