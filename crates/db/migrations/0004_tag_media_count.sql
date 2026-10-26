-- Custom SQL migration file, put your code below! --
-- drizzle-kit doesn't model triggers: keeps tags.media_count in sync with media_tags (also on cascading deletes).
CREATE TRIGGER `media_tags_count_insert` AFTER INSERT ON `media_tags` BEGIN
	UPDATE `tags` SET `media_count` = `media_count` + 1 WHERE `id` = NEW.`tag_id`;
END;--> statement-breakpoint
CREATE TRIGGER `media_tags_count_delete` AFTER DELETE ON `media_tags` BEGIN
	UPDATE `tags` SET `media_count` = `media_count` - 1 WHERE `id` = OLD.`tag_id`;
END;--> statement-breakpoint
UPDATE `tags` SET `media_count` = (SELECT count(*) FROM `media_tags` WHERE `tag_id` = `tags`.`id`);