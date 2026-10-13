/**
 * The command that prints a one-time owner recovery link (docs/deployment.md,
 * "Recovery"). PopcornStash runs in Docker, so the command goes through
 * `docker exec`; the bare `popcornstash admin reset-owner` only works inside
 * the container.
 */
export const OWNER_RECOVERY_COMMAND = 'docker exec popcornstash popcornstash admin reset-owner';
