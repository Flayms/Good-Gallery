-- A face the user took out of a group of unknown faces (REQ-032-25): it stays unknown, but no group takes it again.
ALTER TABLE `detected_faces` ADD `ungrouped` integer DEFAULT 0 NOT NULL;
