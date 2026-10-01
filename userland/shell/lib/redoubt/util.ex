defmodule Redoubt.Util do
  @moduledoc """
  The text toolkit: files come in through `cat/1` and go out through `w/2`, and everything between
  takes lines and chains with `|>`. Its commands are imported at the prompt.

      cat("app.log") |> grep("error", ignore_case: true) |> count()
      cat("config.txt") |> sub("staging", "prod") |> w("config.txt")

  Everything that returns lines returns a `Redoubt.Util.Lines`, which reads only as far as it is
  consumed. A command that has to see every line first (`sort`, `tail`) reads them all.
  """

  use Redoubt.Commandlet, area: "Text"

  alias Redoubt.Commandlet.UsageError
  alias Redoubt.Util.Lines

  # Lines written to a file in one request.
  @batch 256

  @summary "Read files as lines"
  @help """
  The lines of a file, or of several files one after another, without their line endings. They
  are read as they are consumed, so cat("big.log") |> head(5) reads only the start of the file.
  Every file is checked when cat is called, so a missing one fails there and not later.
  """
  @args paths: "the file, or the files in order"
  @examples [
    {~S'cat("app.log")', "the lines of app.log"},
    {~S'cat(["a.log", "b.log"])', "the lines of a.log, then of b.log"}
  ]
  defcommand cat(paths :: many(path)) do
    Enum.each(paths, &readable!/1)
    paths |> Stream.flat_map(&file_lines/1) |> Lines.new()
  end

  @summary "Keep the lines that match a pattern"
  @help """
  The lines holding the pattern: a string anywhere in the line, or a regex matching it. With
  ignore_case, a string is found whatever its case; a regex says so itself, as ~r/error/i.
  """
  @args lines: "the lines to search",
        pattern: "a string to find, or a regex to match",
        ignore_case: "find a string whatever its case"
  @examples [
    {~S'cat("app.log") |> grep("error")', "the lines of app.log holding error"},
    {~S'cat("app.log") |> grep("error", ignore_case: true)', "and those holding ERROR or Error"},
    {~S'cat("app.log") |> grep(~r/^ERROR \d+/)', "the lines starting ERROR and a number"}
  ]
  defcommand grep(lines :: lines, pattern :: pattern, opts :: flags(ignore_case: boolean)) do
    matches? = matcher(:grep, pattern, opts.ignore_case)
    lines |> Stream.filter(matches?) |> Lines.new()
  end

  @summary "Keep the lines that do not match a pattern"
  @help """
  The lines grep would leave out: those not holding the pattern, a string or a regex.
  """
  @args lines: "the lines to search",
        pattern: "a string, or a regex, whose lines to drop",
        ignore_case: "drop a string whatever its case"
  @examples [{~S'cat("app.log") |> grep_v("INFO")', "every line of app.log but the INFO ones"}]
  defcommand grep_v(lines :: lines, pattern :: pattern, opts :: flags(ignore_case: boolean)) do
    matches? = matcher(:grep_v, pattern, opts.ignore_case)
    lines |> Stream.reject(matches?) |> Lines.new()
  end

  @summary "Keep the first lines"
  @help """
  The first lines, and no more is read: cat("big.log") |> head(5) reads only the start of the
  file.
  """
  @args lines: "the lines to take from", count: "how many to keep"
  @examples [
    {~S'cat("app.log") |> head(5)', "the first five lines of app.log"},
    {~S'ls() |> head()', "the first ten names here"}
  ]
  defcommand head(lines :: lines, count :: integer(min: 0) \\ 10) do
    lines |> Stream.take(count) |> Lines.new()
  end

  @summary "Keep the last lines"
  @help """
  The last lines. Every line is read to find them.
  """
  @args lines: "the lines to take from", count: "how many to keep"
  @examples [{~S'cat("app.log") |> tail(5)', "the last five lines of app.log"}]
  defcommand tail(lines :: lines, count :: integer(min: 0) \\ 10) do
    lines |> Enum.take(-count) |> Lines.new()
  end

  @summary "Count lines"
  @help """
  How many lines there are, or how many items of any list.
  """
  @args lines: "the lines to count"
  @examples [{~S'cat("app.log") |> grep("error") |> count()', "how many lines of app.log hold error"}]
  defcommand count(lines :: lines) do
    Enum.count(lines)
  end

  @summary "Sort lines"
  @help """
  The lines in order: by their bytes, so 10 comes before 9 and Z before a. With numeric, by the
  number each starts with, a line that starts with none counting as 0, and lines that tie in
  their order by bytes. Every line is read first.
  """
  @args lines: "the lines to sort", reverse: "largest first", numeric: "by the number each line starts with"
  @examples [
    {~S'cat("names.txt") |> sort()', "names.txt, sorted"},
    {~S'cat("sizes.txt") |> sort(numeric: true, reverse: true)', "the largest number first"}
  ]
  defcommand sort(lines :: lines, opts :: flags(reverse: boolean, numeric: boolean)) do
    key = if opts.numeric, do: &{leading_number(&1), &1}, else: & &1
    order = if opts.reverse, do: :desc, else: :asc
    lines |> Enum.sort_by(key, order) |> Lines.new()
  end

  @summary "Drop repeated lines"
  @help """
  The lines, with a line the same as the one before it dropped. Only neighbours are compared, so
  sort first to drop every repeat: sort() |> uniq().
  """
  @args lines: "the lines"
  @examples [{~S'cat("names.txt") |> sort() |> uniq()', "each name in names.txt, once"}]
  defcommand uniq(lines :: lines) do
    lines |> Stream.dedup() |> Lines.new()
  end

  @summary "Count repeated lines"
  @help """
  Each run of lines the same, as {how many, the line}, in order. Only neighbours are compared, so
  sort first to count every repeat.
  """
  @args lines: "the lines"
  @examples [{~S'cat("app.log") |> cut(" ", 1) |> sort() |> uniq_c()', "how many lines of each level"}]
  defcommand uniq_c(lines :: lines) do
    lines |> Stream.chunk_by(& &1) |> Enum.map(&{length(&1), hd(&1)})
  end

  @summary "Replace a pattern in every line"
  @help """
  The lines with every match of the pattern replaced: a string, or a regex, whose groups the
  replacement names as \\1, \\2 and on.
  """
  @args lines: "the lines",
        pattern: "a string, or a regex, to replace",
        replacement: "what replaces each match"
  @examples [
    {~S'cat("config.txt") |> sub("staging", "prod")', "config.txt with staging made prod"},
    {~S'cat("app.log") |> sub(~r/ERROR (\d+)/, "E\\1")', "ERROR 12 made E12"}
  ]
  defcommand sub(lines :: lines, pattern :: pattern, replacement :: string) do
    replace =
      case pattern do
        %Regex{} = regex -> &Regex.replace(regex, &1, replacement)
        text -> &String.replace(&1, text, replacement)
      end

    lines |> Stream.map(replace) |> Lines.new()
  end

  @summary "Keep some fields of every line"
  @help """
  Splits each line at the separator, and keeps the fields asked for, numbered from 1, in the
  order asked for, joined by the separator. A field a line does not have is empty.
  """
  @args lines: "the lines",
        separator: "what the fields are split at",
        fields: "the field, or the fields, to keep"
  @examples [
    {~S'cat("users.csv") |> cut(",", 2)', "the second field of every line"},
    {~S'cat("passwd") |> cut(":", [1, 6])', "the first and sixth fields"}
  ]
  defcommand cut(lines :: lines, separator :: string, fields :: many(integer(min: 1))) do
    pick = fn line ->
      parts = String.split(line, separator)
      Enum.map_join(fields, separator, &Enum.at(parts, &1 - 1, ""))
    end

    lines |> Stream.map(pick) |> Lines.new()
  end

  @summary "Write lines to a file"
  @help """
  Writes the lines to path, each ended by a newline, replacing what it held. They are written to
  a new file beside it, which then takes its name, so a failure leaves the old file whole, and a
  file can be read and written back in one line: cat(f) |> sub(...) |> w(f).
  """
  @args lines: "the lines to write", path: "the file to write"
  @examples [
    {~S'cat("config.txt") |> sub("staging", "prod") |> w("config.txt")', "change config.txt in place"},
    {~S'ls() |> w("names.txt")', "the names here, into names.txt"}
  ]
  defcommand w(lines :: lines, path :: path) do
    {temporary, file} = open_beside(path, 5)

    try do
      try do
        write_lines(file, lines)
      after
        File.close(file)
      end

      File.rename!(temporary, path)
    after
      File.rm(temporary)
    end
  end

  # A new file beside path, under a name nobody can guess, made only if nothing has that name:
  # never an existing file, or a link planted to one.
  defp open_beside(path, tries) do
    name = Base.url_encode64(:crypto.strong_rand_bytes(9), padding: false)
    temporary = Path.join(Path.dirname(path), ".#{Path.basename(path)}.#{name}.tmp")

    case File.open(temporary, [:write, :binary, :exclusive]) do
      {:ok, file} -> {temporary, file}
      {:error, :eexist} when tries > 1 -> open_beside(path, tries - 1)
      {:error, reason} -> raise File.Error, reason: reason, action: "write to", path: path
    end
  end

  @summary "Add lines to the end of a file"
  @help """
  Writes the lines to the end of path, each ended by a newline, making the file if it is not
  there.
  """
  @args lines: "the lines to add", path: "the file to add them to"
  @examples [{~S'cat("today.log") |> append("all.log")', "today.log added to all.log"}]
  defcommand append(lines :: lines, path :: path) do
    File.open!(path, [:append, :binary], &write_lines(&1, lines))
    :ok
  end

  @summary "Show a file's bytes"
  @help """
  A file's bytes, sixteen to a line: the offset, each byte in hex, and the bytes that are
  printable ASCII, with a dot for any other. The last line is the file's size. Nothing is drawn as
  what it is, so this is how to look at a file that holds control sequences, or is not text.
  """
  @args path: "the file"
  @examples [
    {~S'hexdump("data.bin")', "the bytes of data.bin"},
    {~S'hexdump("x") |> head(4)', "its first 64"}
  ]
  defcommand hexdump(path :: path) do
    readable!(path)

    path
    |> File.stream!(16)
    |> Stream.transform(fn -> 0 end, &{[hex_row(&2, &1)], &2 + byte_size(&1)}, &{[offset(&1)], &1}, fn _ ->
      :ok
    end)
    |> Lines.new()
  end

  @summary "Hash a file"
  @help """
  A file's hash, in lower-case hex: SHA-256, or SHA-512 when asked. The file is read in blocks,
  so a large one is not held in memory.
  """
  @args path: "the file", algorithm: "the hash"
  @examples [
    {~S'checksum("image.bin")', "its SHA-256"},
    {~S'checksum("image.bin", :sha512)', "its SHA-512"}
  ]
  defcommand checksum(path :: path, algorithm :: one_of([:sha256, :sha512]) \\ :sha256) do
    readable!(path)

    path
    |> File.stream!(65_536)
    |> Enum.reduce(:crypto.hash_init(algorithm), &:crypto.hash_update(&2, &1))
    |> :crypto.hash_final()
    |> Base.encode16(case: :lower)
  end

  # A line test for a pattern: a string anywhere in the line, or a regex.
  defp matcher(_command, %Regex{} = regex, false), do: &Regex.match?(regex, &1)

  defp matcher(command, %Regex{}, true),
    do: raise(UsageError, message: "#{command}: ignore_case is for a string; write a regex as ~r/.../i")

  defp matcher(_command, text, false), do: &String.contains?(&1, text)

  defp matcher(_command, text, true) do
    text = String.downcase(text)
    &String.contains?(String.downcase(&1), text)
  end

  # The number a line starts with, for a numeric sort; 0 when it starts with none.
  defp leading_number(line) do
    case Float.parse(String.trim_leading(line)) do
      {number, _rest} -> number
      :error -> 0.0
    end
  end

  defp write_lines(file, lines) do
    lines
    |> Stream.chunk_every(@batch)
    |> Enum.each(fn batch -> IO.binwrite(file, Enum.map(batch, &[to_string(&1), ?\n])) end)
  end

  defp hex_row(at, bytes) do
    cells = for(<<byte <- bytes>>, do: hex(byte, 2)) ++ List.duplicate("  ", 16 - byte_size(bytes))
    {first, second} = Enum.split(cells, 8)
    ascii = for <<byte <- bytes>>, into: "", do: if(byte in 0x20..0x7E, do: <<byte>>, else: ".")
    "#{offset(at)}  #{Enum.join(first, " ")}  #{Enum.join(second, " ")}  |#{ascii}|"
  end

  defp offset(at), do: hex(at, 8)
  defp hex(n, width), do: n |> Integer.to_string(16) |> String.downcase() |> String.pad_leading(width, "0")

  defp readable!(path) do
    case File.stat(path) do
      {:ok, %File.Stat{type: :directory}} ->
        raise File.Error, reason: :eisdir, action: "read file", path: path

      {:ok, _stat} ->
        path

      {:error, reason} ->
        raise File.Error, reason: reason, action: "read file", path: path
    end
  end

  defp file_lines(path), do: path |> File.stream!() |> Stream.map(&chomp/1)

  defp chomp(line) do
    cond do
      String.ends_with?(line, "\r\n") -> binary_part(line, 0, byte_size(line) - 2)
      String.ends_with?(line, "\n") -> binary_part(line, 0, byte_size(line) - 1)
      true -> line
    end
  end
end
