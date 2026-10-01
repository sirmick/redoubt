defmodule Redoubt.Test.SeedTest do
  use ExUnit.Case, async: true

  import Redoubt.Test.Seed

  @moduletag :tmp_dir

  test "seed lays out the tree, and the names git should not hold", %{tmp_dir: dir} do
    seed(dir)

    assert Enum.sort(File.ls!(dir)) ==
             Enum.sort(["notes.txt", "empty.txt", "data.bin", "evil\e[2J.txt", "docs", "logs"])

    assert Enum.sort(File.ls!(Path.join(dir, "logs"))) == ["app.log", "dos.log", "old"]
    assert File.read!(Path.join(dir, "logs/old/2024.log")) == "old entry\n"
    assert File.read!(Path.join(dir, "data.bin")) == :binary.list_to_bin(Enum.to_list(0..255))
    assert File.read!(Path.join(dir, "logs/dos.log")) == "one\r\ntwo\r\nthree\r\n"
  end

  test "each seeded directory is a copy of its own", %{tmp_dir: dir} do
    a = seed(Path.join(dir, "a"))
    b = seed(Path.join(dir, "b"))
    File.write!(Path.join(a, "notes.txt"), "changed\n")
    assert File.read!(Path.join(b, "notes.txt")) == "first\nsecond\nthird\n"
  end
end
