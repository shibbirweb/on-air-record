/**
 * The same rules the server applies to emails and passwords, checked before sending so a form can say
 * what is wrong next to the field. The server still checks everything; this only saves a round trip.
 */

/** Matches `PASSWORD_LENGTH` in `backend/src/services/auth_service.rs`. */
export const PASSWORD_MIN_LENGTH = 8;
export const PASSWORD_MAX_LENGTH = 128;

/** Deliberately loose, like the server: the email is only a login name, nothing is ever sent to it. */
export function emailProblem(email: string): string | null {
  const trimmed = email.trim();
  const at = trimmed.indexOf('@');
  if (trimmed === '' || at <= 0 || at === trimmed.length - 1 || /\s/.test(trimmed)) {
    return 'Enter an email address.';
  }
  return null;
}

/** Counts characters the way the server does, by code point, so an emoji is one character, not two. */
export function passwordProblem(password: string, confirmation?: string): string | null {
  const length = [...password].length;
  if (length < PASSWORD_MIN_LENGTH) {
    return `Use at least ${PASSWORD_MIN_LENGTH} characters.`;
  }
  if (length > PASSWORD_MAX_LENGTH) {
    return `Use at most ${PASSWORD_MAX_LENGTH} characters.`;
  }
  if (confirmation !== undefined && confirmation !== password) {
    return 'The two passwords do not match.';
  }
  return null;
}

/** Server messages are written to sit mid sentence in a log; a form shows them on their own. */
export function capitalise(message: string): string {
  return message.charAt(0).toUpperCase() + message.slice(1);
}
