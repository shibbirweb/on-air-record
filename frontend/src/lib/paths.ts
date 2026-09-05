/**
 * Presenting filesystem paths that belong to the machine running the service, not the one viewing it.
 *
 * The browser could be on any platform, so the examples shown in the settings page are chosen from the
 * shape of a path the server sent rather than from the viewer's own operating system. Someone on a laptop
 * configuring a Linux server should be shown Linux examples.
 */

export type PathStyle = 'windows' | 'posix';

/** Which convention a path follows, judged by its own shape. */
export function pathStyleOf(samplePath: string): PathStyle {
  return /^[A-Za-z]:[\\/]/.test(samplePath) || samplePath.startsWith('\\\\') ? 'windows' : 'posix';
}

export type PathExamples = {
  style: PathStyle;
  /** A full path anchored at the root of a disk. */
  absolute: string;
  /** A path taken from the data directory. */
  relative: string;
};

/** Concrete examples in the same convention as `samplePath`. */
export function pathExamples(samplePath: string): PathExamples {
  const style = pathStyleOf(samplePath);

  return style === 'windows'
    ? { style, absolute: 'D:\\Recordings\\on-air', relative: 'recordings' }
    : { style, absolute: '/mnt/audio/on-air', relative: 'recordings' };
}
