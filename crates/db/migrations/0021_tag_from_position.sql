-- Places named from a GPS position, not from the file's own tags (spec 042): never written into files.
ALTER TABLE `media_tags` ADD `from_position` integer DEFAULT 0 NOT NULL;
