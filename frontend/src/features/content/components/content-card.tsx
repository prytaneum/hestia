import { DownloadSimpleIcon, HeartIcon, PlusIcon } from '@phosphor-icons/react';
import { createLink } from '@tanstack/react-router';
import { motion } from 'motion/react';
import { useState } from 'react';
import type { ContentProject } from '@/api';
import { contentIcon, contentKindLabel } from '@/components/icons';
import { ListingCard } from '@/components/listing-card';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  ContentInstallDialog,
  ModpackInstallDialog,
} from '@/features/content/install';
import { kindInfo } from '@/features/shared/content/lib';
import { compact } from '@/lib/format';
import { listItem } from '@/lib/motion';
import { m } from '@/paraglide/messages.js';

const MotionLink = createLink(motion.a);

/** A project reads by slug when it has one (nicer URL), else its id. */
export const projectRef = (p: Pick<ContentProject, 'slug' | 'id'>) =>
  p.slug || p.id;

/**
 * A project's identity for selection state and React keys. A slug is unique
 * only within its own platform, so the source is part of the key while
 * `projectRef` stays what the daemon is asked for.
 */
export const selectionKey = (
  p: Pick<ContentProject, 'source' | 'slug' | 'id'>,
) => `${p.source}:${projectRef(p)}`;

export function ContentCard({
  project,
  pinnedVersion,
}: {
  project: ContentProject;
  pinnedVersion?: string;
}) {
  const Icon = contentIcon(project.kind);
  const [installing, setInstalling] = useState(false);

  return (
    <>
      <MotionLink
        variants={listItem}
        to="/browse/$kind/$id"
        params={{ kind: kindInfo[project.kind].slug, id: projectRef(project) }}
        search={{
          source: project.source,
          ...(pinnedVersion ? { version: pinnedVersion } : {}),
        }}
        className="group block outline-none focus-visible:ring-1 focus-visible:ring-ring"
      >
        <ListingCard
          icon={Icon}
          image={project.iconUrl}
          title={project.title}
          author={project.author}
          badges={
            <Badge variant="secondary">
              {contentKindLabel[project.kind]()}
            </Badge>
          }
          description={project.description}
          meta={
            <>
              <span className="inline-flex items-center gap-1">
                <DownloadSimpleIcon className="size-3.5" />
                {compact(project.downloads)}
              </span>
              <span className="inline-flex items-center gap-1">
                <HeartIcon className="size-3.5" />
                {compact(project.follows)}
              </span>
              <span className="truncate">{project.categories.join(', ')}</span>
            </>
          }
          action={
            <Button
              size="xs"
              variant="outline"
              data-icon="inline-start"
              onClick={(e) => {
                e.preventDefault();
                setInstalling(true);
              }}
            >
              <PlusIcon weight="bold" />
              {m['app.action.install']()}
            </Button>
          }
        />
      </MotionLink>

      {/* A modpack builds an entry rather than going into one, so it gets its
          own dialog instead of the shared content one. */}
      {project.kind === 'modpack' ? (
        <ModpackInstallDialog
          project={project}
          pinnedVersion={pinnedVersion}
          open={installing}
          onOpenChange={setInstalling}
        />
      ) : (
        <ContentInstallDialog
          project={project}
          pinnedVersion={pinnedVersion}
          open={installing}
          onOpenChange={setInstalling}
        />
      )}
    </>
  );
}
