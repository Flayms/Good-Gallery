import { sql } from 'drizzle-orm'
import {
    type AnySQLiteColumn,
    index,
    integer,
    primaryKey,
    real,
    sqliteTable,
    text,
    uniqueIndex,
} from 'drizzle-orm/sqlite-core'

// All timestamps are integer milliseconds since the Unix epoch.

export const libraryRoots = sqliteTable('library_roots', {
  id: integer('id').primaryKey({ autoIncrement: true }),
  path: text('path').notNull().unique(),
  label: text('label').notNull(),
  status: text('status', { enum: ['unknown', 'online', 'offline'] })
    .notNull()
    .default('unknown'),
  lastScanAt: integer('last_scan_at'),
})

export const media = sqliteTable(
  'media',
  {
    id: integer('id').primaryKey({ autoIncrement: true }),
    rootId: integer('root_id')
      .notNull()
      .references(() => libraryRoots.id, { onDelete: 'cascade' }),
    relPath: text('rel_path').notNull(),
    fileName: text('file_name').notNull(),
    kind: text('kind', { enum: ['image', 'video'] }).notNull(),
    size: integer('size').notNull(),
    mtime: integer('mtime').notNull(),
    /** Mtime of the `.xmp` sidecar, part of change detection. */
    sidecarMtime: integer('sidecar_mtime'),
    width: integer('width'),
    height: integer('height'),
    /** Seconds. */
    duration: real('duration'),
    takenAt: integer('taken_at'),
    sortDate: integer('sort_date').notNull().generatedAlwaysAs(sql`coalesce(taken_at, mtime)`, { mode: 'virtual' }),
    /** Base64-encoded ThumbHash. */
    thumbhash: text('thumbhash'),
    thumbStatus: text('thumb_status', { enum: ['pending', 'ready', 'error'] })
      .notNull()
      .default('pending'),
  },
  (t) => [
    uniqueIndex('media_root_path_uq').on(t.rootId, t.relPath),
    index('media_date_idx').on(t.sortDate, t.id),
    index('media_root_date_idx').on(t.rootId, t.sortDate, t.id),
    index('media_name_idx').on(t.fileName, t.id),
    // Background thumbnail generation picks pending media newest first.
    index('media_thumb_idx').on(t.thumbStatus, t.sortDate),
  ],
)

export const tags = sqliteTable(
  'tags',
  {
    id: integer('id').primaryKey({ autoIncrement: true }),
    name: text('name').notNull(),
    nameNorm: text('name_norm').notNull().unique(),
    parentId: integer('parent_id').references((): AnySQLiteColumn => tags.id, { onDelete: 'cascade' }),
  },
  (t) => [index('tags_parent_idx').on(t.parentId)],
)

export const mediaTags = sqliteTable(
  'media_tags',
  {
    mediaId: integer('media_id')
      .notNull()
      .references(() => media.id, { onDelete: 'cascade' }),
    tagId: integer('tag_id')
      .notNull()
      .references(() => tags.id, { onDelete: 'cascade' }),
  },
  (t) => [primaryKey({ columns: [t.mediaId, t.tagId] }), index('media_tags_tag_idx').on(t.tagId, t.mediaId)],
)

export type LibraryRoot = typeof libraryRoots.$inferSelect
export type Media = typeof media.$inferSelect
export type Tag = typeof tags.$inferSelect
