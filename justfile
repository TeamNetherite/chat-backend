set dotenv-load

default: launch

surreal:
  surreal start --log debug --user root --pass root -b $NETHERITE_CHAT_SURREALDB_URL file:run/data.db

surreal-shell:
  surreal sql -c ws://$NETHERITE_CHAT_SURREALDB_URL -u root -p root --ns netherite --db chat --pretty

launch:
  NETHERITE_CHAT_CD=./run cargo watch -w src -x run
