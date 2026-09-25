/**
 * Small pieces of the two factor setup that are worth testing on their own.
 */

/**
 * The server's QR code SVG as an image URL.
 *
 * Shown through an `<img>` rather than inserted as markup, because a browser never runs scripts inside an
 * SVG loaded as an image. Percent encoding keeps it readable in the address rather than base64.
 */
export function svgDataUri(svg: string): string {
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

/** The text file offered for download next to the recovery codes, saying what they are and how to use them. */
export function recoveryCodesFile(codes: string[], email: string, createdAt: Date): string {
  return [
    'On Air Record recovery codes',
    `Account: ${email}`,
    `Created: ${createdAt.toISOString().slice(0, 10)}`,
    '',
    'Each code signs you in once, in place of a code from your authenticator app.',
    'Keep them somewhere safe and separate from your phone.',
    '',
    ...codes,
    '',
  ].join('\n');
}
