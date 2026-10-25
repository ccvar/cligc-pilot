#!/bin/sh
# cargo run / tauri dev 的 runner：启动开发版前用固定的自签名证书「Cligc Pilot Dev」重签一遍。
#
# 开发版默认是 ad-hoc 签名，每次重新编译签名都变，钥匙串把它当成新程序，于是每次启动都要输一次
# 登录密码；「始终允许」也只管当次那一份二进制。换成固定证书后签名要求是
# identifier "com.cligc.pilot" + 证书哈希，重新编译不变，点一次「始终允许」以后就不再问。
# 证书没装（其他机器、CI）就照常启动。装证书：scripts/dev-signing-cert.sh
case "$(uname -s):$(basename "$1")" in
  Darwin:cligc-pilot)
    if security find-identity -p codesigning 2>/dev/null | grep -q '"Cligc Pilot Dev"'; then
      codesign -f -s "Cligc Pilot Dev" -i com.cligc.pilot "$1" >/dev/null 2>&1 || echo "dev-sign-run: 重签失败，照常启动" >&2
    fi
    ;;
esac
exec "$@"
