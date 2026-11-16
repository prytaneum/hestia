/** The `plugin.*` channels. */
import { call } from './core/ipc';

/** Answers the question `request` that `plugin` asked (`plugin.question`). */
export async function answer(
  request: string,
  plugin: string,
  accepted: boolean,
): Promise<void> {
  await call('plugin.answer', { request, plugin, accepted });
}
