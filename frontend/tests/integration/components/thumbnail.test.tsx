import { GlobeHemisphereWestIcon } from '@phosphor-icons/react';
import { fireEvent, render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Thumbnail } from '@/components/ui/thumbnail';

const image = (container: HTMLElement) => container.querySelector('img');

describe('a thumbnail with a fallback', () => {
  it('shows the fallback when it has no picture of its own', () => {
    const { container } = render(
      <Thumbnail fallback="instance.png" icon={GlobeHemisphereWestIcon} />,
    );
    expect(image(container)?.getAttribute('src')).toBe('instance.png');
  });

  it('steps down to the fallback, then the glyph, as each fails to load', () => {
    const { container } = render(
      <Thumbnail
        src="world.png"
        fallback="instance.png"
        icon={GlobeHemisphereWestIcon}
      />,
    );
    expect(image(container)?.getAttribute('src')).toBe('world.png');

    fireEvent.error(image(container) as HTMLImageElement);
    expect(image(container)?.getAttribute('src')).toBe('instance.png');

    fireEvent.error(image(container) as HTMLImageElement);
    expect(image(container)).toBeNull();
    expect(container.querySelector('svg')).not.toBeNull();
  });
});
