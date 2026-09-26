/**
 * Release notes as written on GitHub, drawn from parsed blocks rather than HTML, so nothing in a release
 * body can add markup to the page.
 */

import { parseNotes, type Inline } from '@/lib/releaseNotes';

function Inlines({ parts }: { parts: Inline[] }) {
  return (
    <>
      {parts.map((part, index) => {
        switch (part.kind) {
          case 'bold':
            return (
              <strong key={index} className="font-semibold">
                {part.text}
              </strong>
            );
          case 'code':
            return (
              <code key={index} className="bg-muted rounded px-1 font-mono text-[0.85em]">
                {part.text}
              </code>
            );
          case 'link':
            return (
              <a key={index} href={part.href} target="_blank" rel="noreferrer noopener" className="underline">
                {part.text}
              </a>
            );
          default:
            return <span key={index}>{part.text}</span>;
        }
      })}
    </>
  );
}

export function ReleaseNotes({ markdown }: { markdown: string }) {
  const blocks = parseNotes(markdown);
  if (blocks.length === 0) {
    return <p className="text-muted-foreground text-sm">No notes were written for this release.</p>;
  }
  return (
    <div className="space-y-2 text-sm [overflow-wrap:anywhere]">
      {blocks.map((block, index) => {
        if (block.kind === 'heading') {
          return (
            <h4
              key={index}
              className="text-muted-foreground pt-1 text-xs font-semibold tracking-wide uppercase"
            >
              {block.text}
            </h4>
          );
        }
        if (block.kind === 'list') {
          return (
            <ul key={index} className="list-disc space-y-1.5 pl-5">
              {block.items.map((item, itemIndex) => (
                <li key={itemIndex}>
                  <Inlines parts={item} />
                </li>
              ))}
            </ul>
          );
        }
        return (
          <p key={index}>
            <Inlines parts={block.inlines} />
          </p>
        );
      })}
    </div>
  );
}
