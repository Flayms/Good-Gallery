-- The camera, the lens and the GPS position of a media item (spec 040); indexed for the smart album editor's lists and
-- conditions. Filled when files are read again at metadata version 4 (REQ-040-03).
ALTER TABLE `media` ADD `camera` text;
--> statement-breakpoint
ALTER TABLE `media` ADD `lens` text;
--> statement-breakpoint
ALTER TABLE `media` ADD `latitude` real;
--> statement-breakpoint
ALTER TABLE `media` ADD `longitude` real;
--> statement-breakpoint
CREATE INDEX `media_camera_idx` ON `media` (`camera`) WHERE `camera` IS NOT NULL;
--> statement-breakpoint
CREATE INDEX `media_lens_idx` ON `media` (`lens`) WHERE `lens` IS NOT NULL;
