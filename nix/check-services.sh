set -euo pipefail

check_state=$(mktemp -d)
export PGHOST="$check_state/socket"
export PGPORT=55432
export PGUSER=ferret
export PGDATABASE=ferret
export DATABASE_URL="postgres://ferret@127.0.0.1:$PGPORT/ferret"
export FERRET_TEST_IMAP=127.0.0.1:53143
mkdir -p "$PGHOST" "$check_state/mail" "$check_state/run"

stop_check_services() {
  if [[ -n ${dovecot_pid:-} ]]; then
    kill "$dovecot_pid" 2>/dev/null || true
    wait "$dovecot_pid" 2>/dev/null || true
  fi
  pg_ctl -D "$check_state/postgres" -m immediate -w stop >/dev/null 2>&1 || true
  rm -rf "$check_state"
}
# Preserve stdenv's EXIT handler while stopping services on either build outcome.
exitHooks+=(stop_check_services)
failureHooks+=(stop_check_services)

initdb -D "$check_state/postgres" --no-locale --encoding=UTF8 --username=ferret --auth=trust
pg_ctl -D "$check_state/postgres" -l "$check_state/postgres.log" -w start \
  -o "-k $PGHOST -h 127.0.0.1 -p $PGPORT -c shared_preload_libraries=pg_search -c max_connections=200"
createdb ferret

cat > "$check_state/dovecot.conf" <<EOF
dovecot_config_version = 2.4.1
dovecot_storage_version = 2.4.1
base_dir = $check_state/run
state_dir = $check_state/run
log_path = /dev/stderr
protocols = imap
listen = 127.0.0.1
ssl = no
auth_allow_cleartext = yes
default_internal_user = $(id -un)
default_internal_group = $(id -gn)
default_login_user = $(id -un)
first_valid_uid = $(id -u)
mail_driver = maildir
mail_path = $check_state/mail/%{user}
passdb static {
  password = {PLAIN}pass
}
userdb static {
  fields {
    uid = $(id -u)
    gid = $(id -g)
    home = $check_state/mail/%{user}
  }
}
service anvil {
  chroot =
}
service imap-login {
  chroot =
  inet_listener imap {
    port = 53143
  }
  inet_listener imaps {
    port = 0
  }
}
namespace inbox {
  inbox = yes
  mailbox Papierkorb {
    special_use = \\Trash
  }
}
EOF
dovecot -F -c "$check_state/dovecot.conf" &
dovecot_pid=$!
for attempt in $(seq 1 100); do
  if (echo >/dev/tcp/127.0.0.1/53143) 2>/dev/null; then
    break
  fi
  kill -0 "$dovecot_pid"
  if [[ $attempt == 100 ]]; then
    echo 'Dovecot did not become ready' >&2
    exit 1
  fi
  sleep 0.1
done
