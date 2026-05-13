-- Dashboard operational metrics reset for the redesigned operations view.
-- Keep configured endpoints, models, users, API keys, and settings intact.

DELETE FROM request_history;
DELETE FROM endpoint_daily_stats;

UPDATE endpoints
SET
    total_requests = 0,
    successful_requests = 0,
    failed_requests = 0;
