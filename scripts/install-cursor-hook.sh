#!/bin/sh
set -eu
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
export TOKENHELL_HOOK_SOURCE="$SCRIPT_DIR/../hooks/cursor-usage-hook.rb"
exec /usr/bin/ruby <<'RUBY'
require 'json'
require 'fileutils'
require 'shellwords'
require 'tempfile'

home = Dir.home
config_dir = File.join(home, '.cursor')
config_path = File.join(config_dir, 'hooks.json')
base = File.join(home, 'Library/Application Support/io.github.ghostpengu.tokenhell')
bin_dir = File.join(base, 'bin')
script = File.join(bin_dir, 'cursor-usage-hook.rb')
FileUtils.mkdir_p(config_dir, mode: 0700)
FileUtils.mkdir_p(bin_dir, mode: 0700)
# Ensure the watcher can attach even before the first turn.
FileUtils.mkdir_p(File.join(base, 'cursor-hook'), mode: 0700)
config = File.exist?(config_path) ? JSON.parse(File.read(config_path)) : { 'version' => 1, 'hooks' => {} }
raise 'hooks.json must be an object' unless config.is_a?(Hash)
raise 'Unsupported hooks.json version' unless config['version'] == 1
config['hooks'] ||= {}
raise 'hooks must be an object' unless config['hooks'].is_a?(Hash)
config['hooks']['stop'] ||= []
raise 'stop hooks must be an array' unless config['hooks']['stop'].is_a?(Array)
command = Shellwords.escape(script)
entry = { 'command' => command, 'timeout' => 2, 'failClosed' => false }
found = false
config['hooks']['stop'].map! do |hook|
  if hook.is_a?(Hash) && hook['command'] == command
    found = true
    hook.merge('timeout' => 2, 'failClosed' => false)
  else
    hook
  end
end
config['hooks']['stop'] << entry unless found

if File.exist?(config_path)
  backup = "#{config_path}.tokenhell-backup-#{Time.now.strftime('%Y%m%d%H%M%S')}-#{Process.pid}"
  FileUtils.cp(config_path, backup, preserve: true)
end
# Replace the executable and config atomically, preserving all unrelated config values.
[[script, File.binread(ENV.fetch('TOKENHELL_HOOK_SOURCE')), 0700],
 [config_path, JSON.pretty_generate(config) + "\n", 0600]].each do |path, contents, mode|
  Tempfile.create(['.tokenhell-', '.tmp'], File.dirname(path)) do |tmp|
    tmp.binmode
    tmp.write(contents)
    tmp.flush
    tmp.chmod(mode)
    File.rename(tmp.path, path)
  end
end
puts 'Cursor stop hook installed.'
RUBY
