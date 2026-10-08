defmodule Redoubt.Test.Seed do
  @moduledoc """
  The tree the file commands are tested against: real files, copied into a test's own directory.

  `seed/1` copies `test/fixtures/tree` into a directory and adds what git should not hold: a name
  with an escape sequence in it, and a file of every byte. Every test that seeds gets its own copy,
  so a test that changes files changes only its own.

      notes.txt            first, second, third
      empty.txt            nothing
      data.bin             the bytes 0 to 255
      evil\\e[2J.txt       a name that would clear a terminal
      docs/fruit.txt       banana, apple, cherry, apple, 10, 9
      logs/app.log         INFO, ERROR, WARN lines
      logs/dos.log         lines ended by CR LF
      logs/old/2024.log    one line, two directories down
  """

  @fixtures Path.expand("fixtures/tree", __DIR__)

  @doc "Copies the tree into `dir`, and returns `dir`."
  def seed(dir) do
    File.cp_r!(@fixtures, dir)
    File.write!(Path.join(dir, "evil\e[2J.txt"), "boo\n")
    File.write!(Path.join(dir, "data.bin"), :binary.list_to_bin(Enum.to_list(0..255)))
    dir
  end

  @doc """
  A fresh directory on a file system other than the tests' own, for moves across volumes, or
  `nil` where there is none: `/xdev` on beamlet (./test-shell mounts one there), `/dev/shm` on
  Linux. Called from a test, which removes it when it ends.
  """
  def other_volume do
    case Enum.find(["/xdev", "/dev/shm"], &File.dir?/1) do
      nil ->
        nil

      base ->
        dir = Path.join(base, "redoubt-shell-#{System.unique_integer([:positive])}")
        File.mkdir_p!(dir)
        ExUnit.Callbacks.on_exit(fn -> File.rm_rf(dir) end)
        dir
    end
  end
end

# The terminal the encoder's tests draw on.
Code.require_file("support/terminal.exs", __DIR__)

ExUnit.start()
