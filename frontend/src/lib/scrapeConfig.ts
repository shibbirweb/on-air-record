/**
 * The Prometheus scrape config the Monitoring card offers to paste.
 *
 * Built from the address the page was opened on, which is the best guess at how another machine on the
 * network reaches this recorder. The port is always written out: a browser drops the default one from the
 * address, and a target without a port is the first thing to go wrong in a scrape config. The token is
 * read from a file rather than written into the config, so the config can be shared or committed without
 * handing out the key.
 */

/** Where the snippet tells Prometheus to read the scrape token from. */
export const TOKEN_FILE = '/etc/prometheus/on-air-record.token';

export function metricsUrl(origin: string): string {
  return `${origin}/api/metrics`;
}

/** The scheme and `host:port` a scraper should use for this recorder. */
export function metricsTarget(origin: string): { scheme: 'http' | 'https'; target: string } {
  const url = new URL(origin);
  const scheme = url.protocol === 'https:' ? 'https' : 'http';
  const port = url.port || (scheme === 'https' ? '443' : '80');
  return { scheme, target: `${url.hostname}:${port}` };
}

/** A `scrape_configs` block, with the token file when the scraper has to send one. */
export function scrapeConfig(origin: string, withToken: boolean): string {
  const { scheme, target } = metricsTarget(origin);
  const lines = [
    'scrape_configs:',
    '  - job_name: on-air-record',
    '    metrics_path: /api/metrics',
    `    scheme: ${scheme}`,
    '    static_configs:',
    `      - targets: ['${target}']`,
  ];
  if (withToken) {
    lines.push('    authorization:', '      type: Bearer', `      credentials_file: ${TOKEN_FILE}`);
  }
  return `${lines.join('\n')}\n`;
}
