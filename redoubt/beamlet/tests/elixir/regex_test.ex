defmodule RegexTest do
  # Elixir's Regex (on re), String functions that take patterns, and Logger.
  require Logger

  def start do
    Logger.info("this goes to standard_error")
    r = ~r/(?<year>\d{4})-(?<month>\d{2})/
    {Regex.run(r, "on 2026-09 and 1999-12"),
     Regex.scan(r, "on 2026-09 and 1999-12"),
     Regex.named_captures(r, "2026-09"),
     Regex.replace(~r/\s+/, "a  b   c", "_"),
     String.split("a1b22c", ~r/\d+/),
     Regex.match?(~r/^hello$/i, "HeLLo"),
     String.replace("hello world", ~r/o/, "0"),
     Regex.split(~r/,\s*/, "a, b,c", trim: true),
     Regex.source(r),
     Regex.names(r),
     String.match?("abc123", ~r/^[a-z]+\d+$/u)}
  end
end
