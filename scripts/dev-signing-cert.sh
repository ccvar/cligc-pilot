#!/bin/sh
# 在登录钥匙串里装一张只给开发版签名用的自签名证书「Cligc Pilot Dev」（只允许 codesign 使用私钥）。
# 装一次就行；scripts/dev-sign-run.sh 会在每次 cargo run / tauri dev 时用它重签开发版。
set -e
NAME="Cligc Pilot Dev"
if security find-identity -p codesigning 2>/dev/null | grep -q "\"$NAME\""; then
  echo "已经装过「$NAME」"; exit 0
fi
D=$(mktemp -d); trap 'rm -rf "$D"' EXIT
cat > "$D/cert.cnf" <<CNF
[req]
distinguished_name = dn
prompt = no
x509_extensions = ext
[dn]
CN = $NAME
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
CNF
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$D/key.pem" -out "$D/cert.pem" -days 3650 -config "$D/cert.cnf" 2>/dev/null
P=$(openssl rand -hex 12)
# macOS 的 security 读不了 OpenSSL 3 默认的 p12 加密方式，要 -legacy
openssl pkcs12 -export -legacy -inkey "$D/key.pem" -in "$D/cert.pem" -out "$D/dev.p12" -passout "pass:$P" -name "$NAME"
security import "$D/dev.p12" -k "$HOME/Library/Keychains/login.keychain-db" -P "$P" -T /usr/bin/codesign
echo "已装好「$NAME」。下次启动开发版点一次「始终允许」，之后重新编译也不会再问。"
