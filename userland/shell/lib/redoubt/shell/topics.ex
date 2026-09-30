defmodule Redoubt.Shell.Topics do
  @moduledoc """
  The help topics: short pages about the shell rather than one command, read with `help(:name)`.

  Each is a Markdown file in `userland/shell/help/`, named for its topic and starting with a
  `# Title` line. They are read when the shell is compiled and kept in this module, so they ship
  inside it and need no file on the machine that runs it. Adding a file is adding a topic; its
  name must not be a command's, and a test holds them apart.
  """

  @dir Path.expand("../../../help", __DIR__)
  @paths @dir |> Path.join("*.md") |> Path.wildcard() |> Enum.sort()

  for path <- @paths, do: @external_resource(path)

  @topics Map.new(@paths, fn path -> {Path.basename(path, ".md"), File.read!(path)} end)

  # A topic added or removed is noticed on the next build, as a changed one is through
  # @external_resource.
  @doc false
  def __mix_recompile__?, do: @dir |> Path.join("*.md") |> Path.wildcard() |> Enum.sort() != @paths

  @doc "Every topic, as {name, title}, by name."
  @spec all() :: [{String.t(), String.t()}]
  def all, do: @topics |> Enum.map(fn {name, text} -> {name, title(text)} end) |> Enum.sort()

  @doc "The topic's text, by its name as a string."
  @spec fetch(String.t()) :: {:ok, String.t()} | :error
  def fetch(name) when is_binary(name), do: Map.fetch(@topics, name)

  defp title("# " <> rest), do: rest |> String.split("\n", parts: 2) |> hd()
  defp title(_text), do: ""
end
