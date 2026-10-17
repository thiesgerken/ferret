-- The config's writeOnly fields, sealed under FERRET_CONFIG_KEY (nonce, then ciphertext). Null when
-- the config has none.
alter table connections add column credentials bytea;
