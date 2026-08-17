CREATE SCHEMA IF NOT EXISTS homelab;

CREATE TABLE homelab.machines (
    id uuid PRIMARY KEY,
    display_name text NOT NULL UNIQUE,
    ssh_host text NOT NULL,
    ssh_port integer NOT NULL DEFAULT 22,
    ssh_username text NOT NULL DEFAULT 'homelab',
    pinned_host_public_key text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT machines_display_name_length CHECK (char_length(display_name) BETWEEN 1 AND 100),
    CONSTRAINT machines_ssh_host_length CHECK (char_length(ssh_host) BETWEEN 1 AND 253),
    CONSTRAINT machines_ssh_port_fixed CHECK (ssh_port = 22),
    CONSTRAINT machines_ssh_username_fixed CHECK (ssh_username = 'homelab'),
    CONSTRAINT machines_pinned_host_public_key_length CHECK (
        pinned_host_public_key IS NULL
        OR char_length(pinned_host_public_key) BETWEEN 1 AND 256
    )
);

CREATE INDEX machines_display_name_id_idx
    ON homelab.machines (display_name, id);
