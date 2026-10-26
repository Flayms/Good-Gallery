-- Undecodable embedded previews (DJI) used to fail the whole render; retry those thumbnails.
UPDATE `media` SET `thumb_status` = 'pending' WHERE `thumb_status` = 'error';