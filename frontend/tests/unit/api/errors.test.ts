import { describe, expect, it } from 'vitest';
import type { ErrorInfo } from '@/api';
import { errorMessage, errorMessageFromInfo, HestiaError } from '@/api';

const info = (value: Record<string, unknown>) => value as unknown as ErrorInfo;

describe('errorMessageFromInfo', () => {
  it('interpolates the variant’s own fields', () => {
    const rendered = errorMessageFromInfo(
      info({ kind: 'account_not_found', reference: 'steve' }),
    );
    expect(rendered).toBe("No account matches 'steve'.");
  });

  it('labels a token field through the shared vocabulary', () => {
    const rendered = errorMessageFromInfo(
      info({ kind: 'already_exists', entry: 'server', name: 'smp' }),
    );
    expect(rendered).toContain('smp');
    expect(rendered).not.toContain('{entry}');
  });

  it('joins a list field into one readable value, labelling each token', () => {
    const rendered = errorMessageFromInfo(
      info({ kind: 'mutually_exclusive', options: ['project', 'url'] }),
    );
    expect(rendered).toContain('project, download URL');
  });

  it('words a closed reason instead of interpolating prose', () => {
    const rendered = errorMessageFromInfo(
      info({ kind: 'archive_invalid', format: 'zip', reason: 'not_an_archive' }),
    );
    expect(rendered).toBe(
      'This zip archive could not be read: the file is incomplete or not a zip archive.',
    );
  });

  it('words a reason that carries data with that data', () => {
    const rendered = errorMessageFromInfo(
      info({
        kind: 'config_invalid',
        key: 'view-distance',
        reason: { out_of_range: { min: 3, max: 32 } },
      }),
    );
    expect(rendered).toBe(
      "Invalid value for 'view-distance': it must be between 3 and 32.",
    );
  });

  it('is empty for a kind the catalogue does not know', () => {
    expect(errorMessageFromInfo(info({ kind: 'not_a_real_kind' }))).toBe('');
  });
});

describe('errorMessage', () => {
  it('prefers the structured variant message', () => {
    const error = new HestiaError(
      'handler_error',
      'raw',
      info({ kind: 'account_not_found', reference: 'steve' }),
    );
    expect(errorMessage(error)).toBe("No account matches 'steve'.");
  });

  it('falls back to the coarse code when the variant is unknown', () => {
    const error = new HestiaError('not_found', 'raw', info({ kind: 'nope' }));
    expect(errorMessage(error)).not.toBe('raw');
    expect(errorMessage(error)).not.toBe('');
  });

  it('falls back to the code when there is no structured info at all', () => {
    expect(errorMessage(new HestiaError('timeout', 'raw'))).not.toBe('raw');
  });

  it('passes a plain Error through untouched', () => {
    expect(errorMessage(new Error('boom'))).toBe('boom');
  });

  it('stringifies anything else that was thrown', () => {
    expect(errorMessage('boom')).toBe('boom');
    expect(errorMessage(42)).toBe('42');
  });
});
