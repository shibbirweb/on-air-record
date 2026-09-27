/**
 * The frontend half of the wire contract: the golden fixtures in `contracts/` at the repository root, and a
 * way to prove a TypeScript type has exactly the shape of the JSON in them.
 *
 * The backend writes most of those files from its own serialisers (`backend/src/contract_tests.rs`) and
 * fails when they drift from what it sends, so here they stand for the server. A type is described by a
 * `Shape`, an object literal listing every key of the type with the kind of value it holds, and the
 * `Shape<T>` type is computed from `T` itself. An object literal of that type must name every key and no
 * others, and give each the kind the type calls for (`'number'`, `{ nullable: 'string' }`, a nested
 * `{ object: ... }`), so the compiler keeps the description honest and the runtime check below compares it
 * with the fixture. A key renamed on either side then fails one or the other.
 *
 * The files are imported with Vite's `?raw` suffix rather than read with `node:fs`. The app's tsconfig
 * includes only the browser's types, and pulling Node's in for one test would type `process` and
 * `Buffer` for every file in the app.
 */

import audioFramesText from '../../../contracts/audio-frames.json?raw';
import clientMessagesText from '../../../contracts/client-messages.json?raw';
import enumsText from '../../../contracts/enums.json?raw';
import requestsText from '../../../contracts/requests.json?raw';
import responsesText from '../../../contracts/responses.json?raw';
import serverMessagesText from '../../../contracts/server-messages.json?raw';

export type Json = Record<string, unknown>;

/** Examples grouped under a name: a message type, a response, or an `api` method. */
export type Examples<T = Json> = Record<string, T[]>;

export type FrameFixture = {
  hex: string;
  byteLength: number;
  header: {
    magic: number;
    version: number;
    formatCode: number;
    channels: number;
    flags: number;
    live: boolean;
    sampleRate: number;
    sampleCount: number;
    timestampMs: number;
  };
  samples: number[];
};

export type BodyFixture = { method: string; path: string; body: Json };

export type QueryFixture = { path: string; query: Record<string, string | number> };

export const fixtures = {
  serverMessages: JSON.parse(serverMessagesText) as Examples,
  responses: JSON.parse(responsesText) as Examples,
  enums: JSON.parse(enumsText) as Record<string, string[]>,
  audioFrames: JSON.parse(audioFramesText) as Record<string, FrameFixture>,
  clientMessages: JSON.parse(clientMessagesText) as Examples,
  requests: JSON.parse(requestsText) as {
    bodies: Examples<BodyFixture>;
    queries: Examples<QueryFixture>;
  },
};

type PlainFieldFor<V> = [V] extends [string]
  ? 'string'
  : [V] extends [number]
    ? 'number'
    : [V] extends [boolean]
      ? 'boolean'
      : [V] extends [readonly (infer E)[]]
        ? { array: FieldFor<E> }
        : [V] extends [object]
          ? { object: Shape<V> }
          : never;

/** How a value of type `V` must be described: its kind, wrapped in `nullable` when `V` admits null. */
export type FieldFor<V> = null extends V
  ? { nullable: PlainFieldFor<Exclude<V, null>> }
  : PlainFieldFor<V>;

/**
 * Every key of `T`, optional ones included, each with the description its type calls for. Written as an
 * object literal, the compiler rejects a missing key, an extra key, and a wrong kind.
 */
export type Shape<T> = { [P in keyof T]-?: FieldFor<Exclude<T[P], undefined>> };

export type AnyField =
  | 'string'
  | 'number'
  | 'boolean'
  | { nullable: AnyField }
  | { array: AnyField }
  | { object: AnyShape };

export type AnyShape = { readonly [key: string]: AnyField };

/** Describe `T`, checked by the compiler, for a table that holds shapes of many types. */
export function shape<T>(fields: Shape<T>): AnyShape {
  return fields as unknown as AnyShape;
}

/** Every member of a string union, as an object literal the compiler holds to exactly that union. */
export function members<T extends string>(table: Record<T, true>): string[] {
  return Object.keys(table).sort();
}

/**
 * What the fixtures have shown, so a check can insist every nullable field was seen both null and set,
 * and every array seen with something in it. A field never seen null proves nothing about `| null`, and
 * an empty array proves nothing about what goes in it.
 */
export class Coverage {
  private readonly nullable = new Set<string>();
  private readonly seenNull = new Set<string>();
  private readonly seenValue = new Set<string>();
  private readonly arrays = new Set<string>();
  private readonly filledArrays = new Set<string>();

  nullableAt(path: string, isNull: boolean): void {
    this.nullable.add(path);
    if (isNull) {
      this.seenNull.add(path);
    } else {
      this.seenValue.add(path);
    }
  }

  arrayAt(path: string, length: number): void {
    this.arrays.add(path);
    if (length > 0) {
      this.filledArrays.add(path);
    }
  }

  /** Everything the fixtures never exercised, as sentences for a failure message. */
  gaps(): string[] {
    const gaps: string[] = [];
    for (const path of [...this.nullable].sort()) {
      if (!this.seenNull.has(path)) {
        gaps.push(
          `${path}: the type allows null but no fixture is null there. If the server never sends null, ` +
            'drop `| null` from the type; if it can, give the backend fixture an example with None.',
        );
      }
      if (!this.seenValue.has(path)) {
        gaps.push(`${path}: every fixture is null there, so the value's own type is never checked.`);
      }
    }
    for (const path of [...this.arrays].sort()) {
      if (!this.filledArrays.has(path)) {
        gaps.push(`${path}: every fixture has an empty array there, so its elements are never checked.`);
      }
    }
    return gaps;
  }
}

function kindOf(value: unknown): string {
  if (value === null) {
    return 'null';
  }
  if (Array.isArray(value)) {
    return 'array';
  }
  return typeof value;
}

function isObject(value: unknown): value is Json {
  return kindOf(value) === 'object';
}

/**
 * Compare one fixture value with the description of its type, recording every difference in `problems`
 * rather than stopping at the first, so one run reports the whole of a drift.
 */
export function checkValue(
  value: unknown,
  field: AnyField,
  path: string,
  coverage: Coverage,
  problems: string[],
): void {
  if (typeof field === 'string') {
    const kind = kindOf(value);
    if (kind !== field) {
      problems.push(`${path}: the type says ${field}, the server sends ${kind} (${JSON.stringify(value)})`);
    } else if (field === 'number' && !Number.isFinite(value)) {
      problems.push(`${path}: not a finite number`);
    }
    return;
  }

  if ('nullable' in field) {
    coverage.nullableAt(path, value === null);
    if (value !== null) {
      checkValue(value, field.nullable, path, coverage, problems);
    }
    return;
  }

  if ('array' in field) {
    if (!Array.isArray(value)) {
      problems.push(`${path}: the type says an array, the server sends ${kindOf(value)}`);
      return;
    }
    coverage.arrayAt(path, value.length);
    value.forEach((item) => checkValue(item, field.array, `${path}[]`, coverage, problems));
    return;
  }

  checkObject(value, field.object, path, coverage, problems);
}

/** The keys of the value and of the shape must be the same set, and each value must fit its field. */
export function checkObject(
  value: unknown,
  fields: AnyShape,
  path: string,
  coverage: Coverage,
  problems: string[],
): void {
  if (!isObject(value)) {
    problems.push(`${path}: the type says an object, the server sends ${kindOf(value)}`);
    return;
  }

  const expected = Object.keys(fields).sort();
  const actual = Object.keys(value).sort();
  const unknownToType = actual.filter((key) => !expected.includes(key));
  const neverSent = expected.filter((key) => !actual.includes(key));
  if (unknownToType.length > 0) {
    problems.push(`${path}: the server sends ${unknownToType.join(', ')}, which the type does not have`);
  }
  if (neverSent.length > 0) {
    problems.push(`${path}: the type has ${neverSent.join(', ')}, which the server does not send`);
  }

  for (const key of expected.filter((name) => actual.includes(name))) {
    checkValue(value[key], fields[key], `${path}.${key}`, coverage, problems);
  }
}

/**
 * Check a partial object, such as a settings patch: every key it has must be one the type has, with the
 * right kind, but it need not have them all. The caller checks that the examples together use every key.
 */
export function checkPartial(
  value: unknown,
  fields: AnyShape,
  path: string,
  coverage: Coverage,
  problems: string[],
): void {
  if (!isObject(value)) {
    problems.push(`${path}: expected an object, found ${kindOf(value)}`);
    return;
  }
  for (const key of Object.keys(value)) {
    const field = fields[key];
    if (!field) {
      problems.push(`${path}: ${key} is sent, but the type does not have it`);
      continue;
    }
    checkValue(value[key], field, `${path}.${key}`, coverage, problems);
  }
}

/** The difference between two sets of names, as a readable failure rather than two long arrays. */
export function sameNames(what: string, typeSide: string[], fixtureSide: string[]): string[] {
  const problems: string[] = [];
  const missingFixture = typeSide.filter((name) => !fixtureSide.includes(name));
  const missingType = fixtureSide.filter((name) => !typeSide.includes(name));
  if (missingFixture.length > 0) {
    problems.push(`${what}: no fixture for ${missingFixture.join(', ')}`);
  }
  if (missingType.length > 0) {
    problems.push(`${what}: the fixtures have ${missingType.join(', ')}, which the frontend does not`);
  }
  return problems;
}
