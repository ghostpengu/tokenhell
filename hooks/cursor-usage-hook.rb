#!/usr/bin/ruby
# Cursor sends JSON on stdin (a shell heredoc). Persist only small usage fields.
# Record CLI builds only. The IDE fires the same user-level stop hook and those
# turns already arrive through the dashboard sync, so writing them would count twice.
require 'json'
require 'fileutils'

def label(value)
  return nil unless value.is_a?(String)
  text = value.strip
  return nil if text.empty? || text.length > 128
  return nil if text =~ /[\x00-\x1f\x7f]/
  text
end

def count(value)
  number = case value
           when Integer then value
           when Float
             return nil unless value.finite? && value >= 0 && value == value.floor
             value.to_i
           when String
             return nil unless value =~ /\A\d{1,19}\z/
             value.to_i
           else
             return nil
           end
  return nil if number < 0 || number > 9_223_372_036_854_775_807
  number
end

begin
  raw = STDIN.read(1_048_577)
  raise 'payload too large' if raw.nil? || raw.bytesize > 1_048_576
  data = JSON.parse(raw)
  version = data['cursor_version'] if data.is_a?(Hash)
  cli = version.is_a?(String) && version =~ /\A\d{4}\.\d{2}\.\d{2}-[A-Za-z0-9]+\z/
  if cli && data['hook_event_name'] == 'stop'
    id_pattern = /\A[a-zA-Z0-9_.-]{1,256}\z/
    conversation = data['conversation_id']
    generation = data['generation_id']
    if conversation.is_a?(String) && conversation =~ id_pattern &&
       generation.is_a?(String) && generation =~ id_pattern
      record = { 'ts' => (Time.now.to_f * 1000).to_i,
                 'conversation_id' => conversation, 'generation_id' => generation }
      slug = label(data['model_id'])
      name = label(data['model'])
      model = slug && slug !~ /\s/ ? slug : name
      record['model'] = model if model
      status = data['status']
      record['status'] = status if status.is_a?(String) && status =~ id_pattern
      positive = false
      %w[input_tokens output_tokens cache_read_tokens cache_write_tokens].each do |key|
        number = count(data[key])
        next if number.nil?
        record[key] = number
        positive ||= number > 0
      end
      if positive
        dir = File.join(Dir.home, 'Library/Application Support/io.github.ghostpengu.tokenhell/cursor-hook')
        FileUtils.mkdir_p(dir, mode: 0700)
        File.open(File.join(dir, 'usage.jsonl'), File::WRONLY | File::APPEND | File::CREAT, 0600) do |file|
          file.flock(File::LOCK_EX)
          file.syswrite(JSON.generate(record) + "\n")
        end
      end
    end
  end
rescue Exception
  # Observational: errors must never disrupt the agent or expose the payload.
ensure
  begin
    STDOUT.write("{}\n")
  rescue Exception
  end
end
exit 0
