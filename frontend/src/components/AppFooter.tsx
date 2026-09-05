/**
 * Attribution, licence, and where to report a fault.
 *
 * The version is here rather than hidden in a dialogue because it is the first thing anybody asks for in a
 * bug report, and this is the page the reporter already has open.
 */

import { Bug, Star } from 'lucide-react';
import { useEffect } from 'react';

import { useStatusStore } from '@/store/useStatusStore';

const AUTHOR = 'MD. Shibbir Ahmed';
const PORTFOLIO_URL = 'https://shibbirweb.github.io';
const REPOSITORY_URL = 'https://github.com/shibbirweb/on-air-record';
const ISSUES_URL = `${REPOSITORY_URL}/issues`;

/** Resolved once at load. The page is never open long enough for a new year to matter. */
const YEAR = new Date().getFullYear();

const LINK = 'hover:text-foreground underline-offset-4 transition-colors hover:underline';

export function AppFooter() {
  const version = useStatusStore((state) => state.version);
  const loadVersion = useStatusStore((state) => state.loadVersion);

  // Once, not polled: the version cannot change without the process restarting, which drops the socket
  // and reloads the page anyway.
  useEffect(() => {
    void loadVersion();
  }, [loadVersion]);

  return (
    <footer className="mt-2 border-t">
      <div className="text-muted-foreground mx-auto flex max-w-[1600px] flex-wrap items-center gap-x-3 gap-y-1 px-4 py-4 text-xs">
        <span>
          &copy; {YEAR}{' '}
          <a href={PORTFOLIO_URL} target="_blank" rel="noreferrer noopener" className={LINK}>
            {AUTHOR}
          </a>
        </span>

        <span aria-hidden="true" className="hidden sm:inline">&middot;</span>
        <span>MIT licence</span>

        <span aria-hidden="true" className="hidden sm:inline">&middot;</span>
        {/* Still the repository link, but asking for the thing worth asking for. */}
        <a
          href={REPOSITORY_URL}
          target="_blank"
          rel="noreferrer noopener"
          className={`${LINK} flex items-center gap-1.5`}
        >
          <Star className="size-3.5" />
          Star the repository
        </a>

        <a
          href={ISSUES_URL}
          target="_blank"
          rel="noreferrer noopener"
          className={`${LINK} flex items-center gap-1.5 sm:ml-auto`}
        >
          <Bug className="size-3.5" />
          Something wrong? Report an issue
        </a>

        {version !== null && (
          <span className="tabular" title="The version to quote in a bug report">
            v{version}
          </span>
        )}
      </div>
    </footer>
  );
}
