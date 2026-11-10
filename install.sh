#!/bin/sh
# Install verified release companions without a compiler or language runtime.
set -eu
fail() { printf 'Forge installer: %s\n' "$*" >&2; exit 1; }
[ "$#" -eq 0 ] || fail 'Use FORGE_RELEASE_BASE_URL to select a release asset directory.'
case $(uname -s) in Linux) os=unknown-linux-gnu ;; Darwin) os=apple-darwin ;; *) fail 'Unsupported operating system' ;; esac
case $(uname -m) in x86_64) arch=x86_64 ;; aarch64|arm64) arch=aarch64 ;; *) fail 'Unsupported architecture' ;; esac
asset=contextunity-forge-mcp-$arch-$os.tar.gz
base=${FORGE_RELEASE_BASE_URL:-https://github.com/ContextUnity/contextunity-forge-mcp/releases/latest/download}
case $HOME in /*) ;; *) fail 'HOME must be an absolute path' ;; esac
bin_dir=$HOME/.local/bin
binary=$bin_dir/contextunity-forge-mcp
[ ! -e "$binary" ] || [ -f "$binary" ] || fail 'Executable destination must be a regular file, not a directory or special file'
config=$PWD/.mcp.json
[ ! -L "$config" ] || fail 'Refusing symlink .mcp.json'
if [ -e "$config" ] && [ ! -f "$config" ]; then fail '.mcp.json must be a regular file'; fi
umask 077
tmp=$(mktemp -d "${TMPDIR:-/tmp}/forge-install.XXXXXXXX")
binary_stage=
config_stage=
cleanup() {
    rm -rf "$tmp"
    [ -z "$binary_stage" ] || rm -f "$binary_stage"
    [ -z "$config_stage" ] || rm -f "$config_stage"
}
trap cleanup 0
trap 'exit 1' HUP INT TERM
download() {
    if command -v curl >/dev/null 2>&1; then curl -fsSL "$1" -o "$2"
    elif command -v wget >/dev/null 2>&1; then wget -q -O "$2" "$1"
    else fail 'curl or wget is required'; fi
}
download "${base%/}/$asset" "$tmp/$asset"
download "${base%/}/SHA256SUMS" "$tmp/SHA256SUMS"
expected=$(awk -v name="$asset" '$2 == name && NF == 2 && length($1) == 64 && $1 !~ /[^0-9a-f]/ { digest=$1; count++ } END { if(count != 1) exit 1; print digest }' "$tmp/SHA256SUMS") || fail 'Missing or ambiguous archive checksum'
if command -v sha256sum >/dev/null 2>&1; then actual=$(sha256sum "$tmp/$asset")
elif command -v shasum >/dev/null 2>&1; then actual=$(shasum -a 256 "$tmp/$asset")
else fail 'sha256sum or shasum is required'; fi
actual=${actual%% *}
[ "$actual" = "$expected" ] || fail 'Archive SHA256 checksum mismatch'
mkdir "$tmp/release"
tar -xzf "$tmp/$asset" -C "$tmp/release"
[ -f "$tmp/release/contextunity-forge-mcp" ] && [ ! -L "$tmp/release/contextunity-forge-mcp" ] || fail 'Archive has no regular executable'
[ -f "$tmp/release/skills/install.sh" ] || fail 'Archive has no companion skill installer'
# Parse complete JSON before durable changes; only the two owned Forge fields
# change. Decode member names so Unicode-escaped keys have the same ownership.
if [ -f "$config" ]; then cp "$config" "$tmp/client.json"; else printf '{}\n' > "$tmp/client.json"; fi
LC_ALL=C FORGE_INSTALL_COMMAND=$binary awk '
function bad() { failed=1; exit 1 }
function utf8(s, i,b,n,j,lo,hi,c) {
    for(i=1;i<=length(s);i++) {
        b=bytes[substr(s,i,1)]
        if(b<128) continue
        lo=128; hi=191
        if(b>=194 && b<=223) n=1
        else if(b>=224 && b<=239) { n=2; if(b==224) lo=160; if(b==237) hi=159 }
        else if(b>=240 && b<=244) { n=3; if(b==240) lo=144; if(b==244) hi=143 }
        else bad()
        if(i+n>length(s)) bad()
        for(j=1;j<=n;j++) {
            c=bytes[substr(s,i+j,1)]
            if(c<(j==1 ? lo : 128) || c>(j==1 ? hi : 191)) bad()
        }
        i+=n
    }
}
function control(c, i) { for(i=0;i<32;i++) if(c==sprintf("%c",i)) return i; return -1 }
function ws() { while (substr(doc,p,1) ~ /[ \t\r\n]/ && p <= length(doc)) p++ }
function hex(s, i,n,c) { n=0; if(length(s)!=4 || s ~ /[^0-9a-fA-F]/) bad(); for(i=1;i<=4;i++){ c=index("0123456789abcdef",tolower(substr(s,i,1)))-1; n=n*16+c } return n }
function str( start,c,e,n,lo,decoded) {
    start=p; if(substr(doc,p++,1)!="\"") bad(); decoded=""
    while(p<=length(doc)) {
        c=substr(doc,p++,1)
        if(c=="\""){ key=decoded; return substr(doc,start,p-start) }
        if(control(c)>=0) bad()
        if(c!="\\"){ decoded=decoded c; continue }
        e=substr(doc,p++,1)
        if(e=="u") {
            n=hex(substr(doc,p,4)); p+=4
            if(n>=55296 && n<=56319){ if(substr(doc,p,2)!="\\u") bad(); lo=hex(substr(doc,p+2,4)); if(lo<56320 || lo>57343) bad(); p+=6 }
            else if(n>=56320 && n<=57343) bad()
            decoded=decoded (n>0 && n<128 ? sprintf("%c",n) : "?")
        } else if(e ~ /^["\\\/]$/) decoded=decoded e
        else if(e=="b") decoded=decoded sprintf("%c",8)
        else if(e=="f") decoded=decoded sprintf("%c",12)
        else if(e=="n") decoded=decoded "\n"
        else if(e=="r") decoded=decoded "\r"
        else if(e=="t") decoded=decoded "\t"
        else bad()
    }
    bad()
}
function quote(s, i,c,n,out) {
    out="\""
    for(i=1;i<=length(s);i++) {
        c=substr(s,i,1)
        if(c=="\\" || c=="\"") out=out "\\" c
        else if((n=control(c))>=0) {
            out=out sprintf("\\u%04x",n)
        } else out=out c
    }
    return out "\""
}
function value(mode, c,start,out,k,raw,v,child,seenserver,seenforge,seencmd,seentype,count) {
    ws(); c=substr(doc,p,1)
    if(mode && c!="{") bad()
    if(c=="{") {
        p++; ws(); out="{"; count=0
        if(substr(doc,p,1)!="}") while(1) {
            ws(); raw=str(); k=key; ws(); if(substr(doc,p++,1)!=":") bad()
            child=0
            if(mode==1 && k=="mcpServers"){ if(seenserver++) bad(); child=2 }
            if(mode==2 && k=="contextunity-forge"){ if(seenforge++) bad(); child=3 }
            v=value(child)
            if(mode==3 && k=="command"){ if(seencmd++) bad(); v=quote(ENVIRON["FORGE_INSTALL_COMMAND"]) }
            if(mode==3 && k=="type"){ if(seentype++) bad(); v="\"stdio\"" }
            out=out (count++ ? "," : "") raw ":" v
            ws(); c=substr(doc,p,1); if(c=="}") break
            if(c!=",") bad(); p++
        }
        p++
        if(mode==1 && !seenserver) out=out (count++ ? "," : "") "\"mcpServers\":" freshservers
        if(mode==2 && !seenforge) out=out (count++ ? "," : "") "\"contextunity-forge\":" freshforge
        if(mode==3 && !seencmd) out=out (count++ ? "," : "") "\"command\":" quote(ENVIRON["FORGE_INSTALL_COMMAND"])
        if(mode==3 && !seentype) out=out (count++ ? "," : "") "\"type\":\"stdio\""
        return out "}"
    }
    if(c=="[") {
        p++; ws(); out="["; count=0
        if(substr(doc,p,1)!="]") while(1) {
            v=value(0); out=out (count++ ? "," : "") v; ws(); c=substr(doc,p,1)
            if(c=="]") break; if(c!=",") bad(); p++
        }
        p++; return out "]"
    }
    if(c=="\"") return str()
    start=substr(doc,p)
    if(match(start,/^-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?/)){ out=substr(start,1,RLENGTH); p+=RLENGTH; return out }
    if(match(start,/^(true|false|null)/)){ out=substr(start,1,RLENGTH); p+=RLENGTH; return out }
    bad()
}
BEGIN { for(i=0;i<256;i++) bytes[sprintf("%c",i)]=i }
{ doc=doc $0 "\n" }
END {
    if(failed) exit 1
    utf8(doc); utf8(ENVIRON["FORGE_INSTALL_COMMAND"])
    freshforge="{\"type\":\"stdio\",\"command\":" quote(ENVIRON["FORGE_INSTALL_COMMAND"]) "}"
    freshservers="{\"contextunity-forge\":" freshforge "}"
    p=1; result=value(1); ws(); if(p<=length(doc)) bad(); print result
}' "$tmp/client.json" > "$tmp/client.new" || fail 'Invalid .mcp.json object; configuration is unchanged'
existing=0
[ ! -e "$binary" ] || existing=1
mkdir -p "$bin_dir"
binary_stage=$(mktemp "$bin_dir/.forge-binary.XXXXXXXX")
cp "$tmp/release/contextunity-forge-mcp" "$binary_stage"
chmod 755 "$binary_stage"
sh "$tmp/release/skills/install.sh"
config_stage=$(mktemp "$PWD/.forge-client.XXXXXXXX")
cp "$tmp/client.new" "$config_stage"
mv -f "$binary_stage" "$binary"; binary_stage=
mv -f "$config_stage" "$config"; config_stage=
printf 'Installed %s\n' "$binary"
case :$PATH: in *:"$bin_dir":*) ;; *) printf 'Add %s to PATH for terminal use.\n' "$bin_dir" >&2 ;; esac
if [ "$existing" = 1 ]; then
    if "$binary" reload > "$tmp/reload.log" 2>&1; then cat "$tmp/reload.log"
    else printf 'Reload notice: %s\n' "$(cat "$tmp/reload.log")" >&2; fi
fi
