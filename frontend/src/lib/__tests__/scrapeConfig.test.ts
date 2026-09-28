/**
 * The Prometheus snippet the Monitoring card offers to copy. It is pasted into somebody's config as it is,
 * so what matters is that it names this recorder the way a scraper on another machine can reach it: the
 * right scheme, an explicit port, and the path under `/api`.
 */

import { describe, expect, it } from 'vitest';

import { metricsTarget, metricsUrl, scrapeConfig, TOKEN_FILE } from '../scrapeConfig';

describe('metricsUrl', () => {
  it('is the page address with the API path', () => {
    expect(metricsUrl('http://recorder.local:8080')).toBe('http://recorder.local:8080/api/metrics');
    expect(metricsUrl('https://audio.example.com')).toBe('https://audio.example.com/api/metrics');
  });
});

describe('metricsTarget', () => {
  it('keeps a port the page was opened on', () => {
    expect(metricsTarget('http://192.168.1.20:8080')).toEqual({ scheme: 'http', target: '192.168.1.20:8080' });
  });

  it('spells out the default port, which a browser leaves out of the address', () => {
    expect(metricsTarget('http://recorder.local')).toEqual({ scheme: 'http', target: 'recorder.local:80' });
    expect(metricsTarget('https://audio.example.com')).toEqual({
      scheme: 'https',
      target: 'audio.example.com:443',
    });
  });

  it('keeps the brackets an IPv6 address needs', () => {
    expect(metricsTarget('http://[fd00::20]:8080')).toEqual({ scheme: 'http', target: '[fd00::20]:8080' });
  });
});

describe('scrapeConfig', () => {
  it('scrapes the API path on this recorder, with no credentials when none are needed', () => {
    expect(scrapeConfig('http://recorder.local:8080', false)).toBe(
      [
        'scrape_configs:',
        '  - job_name: on-air-record',
        '    metrics_path: /api/metrics',
        '    scheme: http',
        '    static_configs:',
        "      - targets: ['recorder.local:8080']",
        '',
      ].join('\n'),
    );
  });

  it('reads the token from a file when one is needed, so it never sits in the config itself', () => {
    const config = scrapeConfig('https://audio.example.com', true);
    expect(config).toContain('    scheme: https\n');
    expect(config).toContain("      - targets: ['audio.example.com:443']\n");
    expect(config).toContain(
      ['    authorization:', '      type: Bearer', `      credentials_file: ${TOKEN_FILE}`, ''].join('\n'),
    );
    expect(config.endsWith('\n')).toBe(true);
  });

  it('keeps the file in the Prometheus config folder', () => {
    expect(TOKEN_FILE).toBe('/etc/prometheus/on-air-record.token');
  });
});
