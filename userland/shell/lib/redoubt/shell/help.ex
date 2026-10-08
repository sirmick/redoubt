defmodule Redoubt.Shell.Help do
  @moduledoc "The help commands, imported at the prompt: the shell's own help, and Elixir's."

  use Redoubt.Commandlet, area: "Shell"

  alias Redoubt.Commandlet
  alias Redoubt.Commandlet.{Registry, UsageError}
  alias Redoubt.Shell.Topics
  alias Redoubt.Util.Lines

  @summary "Show the commands, a command's page, or a topic"
  @help """
  With nothing, lists every command by area, one line each, and the topics. With a command's
  name, shows its page: how to call it, each parameter's type and meaning, what it does, and
  examples. With a topic's name, shows the topic. What it shows is lines, so it can be searched:
  help() |> grep("file").
  """
  @args subject: "a command or a topic; every command when left out"
  @examples [
    {"help()", "every command, one line each, and the topics"},
    {"help(:grep)", "grep's page"},
    {"help(:elixir)", "how Elixir reads at the prompt"},
    {~S'help() |> grep("file")', "the commands that mention files"}
  ]
  defcommand help(subject :: name \\ nil) do
    lines =
      cond do
        subject == nil ->
          Commandlet.index(Registry.all()) ++ ["" | footer()]

        match?({:ok, _}, Registry.fetch(subject)) ->
          {:ok, command} = Registry.fetch(subject)
          Commandlet.page(command)

        match?({:ok, _}, Topics.fetch(subject)) ->
          {:ok, text} = Topics.fetch(subject)
          String.split(String.trim_trailing(text), "\n")

        true ->
          raise UsageError,
            message:
              "help: there is no command or topic named #{subject}; help() lists them\nusage: help(subject \\\\ nil)"
      end

    Lines.new(lines, :help)
  end

  @summary "Show Elixir's documentation of a module or a function"
  @help """
  The documentation Elixir keeps with a module, or with a function, named by reference: h(File),
  h(&File.cp/2). A function named at any arity its defaults allow finds its page. A module or
  function that is internal, or has no documentation, says so.
  """
  @args target: "a module, or a function reference such as &File.cp/2"
  @examples [
    {"h(File)", "the File module"},
    {"h(&File.cp/2)", "File.cp/2, and its forms with defaults"},
    {"h(&grep/2)", "the shell's own grep, as Elixir documents it"}
  ]
  defcommand h(target :: ref) do
    target |> docs() |> Lines.new()
  end

  @doc """
  How the pager draws a line of help (`Redoubt.Util.Lines`, style `:help`): `{text, bold,
  indent}`. A Markdown heading is bold without its hashes; so is a line of one capitalized word
  (an area of the index, a page's "Examples") and a page's usage line, `name(params)`. A list
  item's further rows are indented under its text, any other line's under its first character.
  Only the shell's own help is drawn so: nothing else makes lines of this style.
  """
  @spec styled(String.t()) :: {String.t(), boolean(), non_neg_integer()}
  def styled(line) do
    case Regex.run(~r/^\#{1,6} (.*)$/, line, capture: :all_but_first) do
      [heading] -> {heading, true, 0}
      nil -> {line, heading?(line), indent(line)}
    end
  end

  # One capitalized word, or `name(params)`.
  defp heading?(line), do: Regex.match?(~r/^([A-Z][a-z]*|[a-z_]\w*[?!]?\(.*\))$/, line)

  # The leading spaces, and a list item's marker.
  defp indent(line), do: ~r/^ *([-*] )?/ |> Regex.run(line) |> hd() |> byte_size()

  defp footer do
    topics = Enum.map_join(Topics.all(), ", ", fn {name, _title} -> "help(:#{name})" end)
    ["Topics: #{topics}", "Elixir's own documentation: h(File), h(&File.cp/2)"]
  end

  defp docs({:module, module}) do
    case Code.fetch_docs(module) do
      {:docs_v1, _anno, _language, _format, %{"en" => doc}, _meta, _docs} ->
        ["# #{inspect(module)}", "" | text(doc)]

      {:docs_v1, _anno, _language, _format, :hidden, _meta, _docs} ->
        ["#{inspect(module)} is internal: it has no documentation"]

      {:docs_v1, _anno, _language, _format, :none, _meta, _docs} ->
        ["#{inspect(module)} has no documentation"]

      {:error, _reason} ->
        ["#{inspect(module)} has no documentation here: its compiled code carries none"]
    end
  end

  defp docs({:function, module, name, arity}) do
    shown = "#{inspect(module)}.#{name}/#{arity}"

    with {:docs_v1, _anno, _language, _format, _module_doc, _meta, entries} <- Code.fetch_docs(module),
         entry when entry != nil <- Enum.find(entries, &covers?(&1, name, arity)) do
      function_docs(module, shown, entry)
    else
      {:error, _reason} -> ["#{shown} has no documentation here: its compiled code carries none"]
      nil -> ["#{shown} has no documentation"]
    end
  end

  # Whether a docs entry is the function at this arity: its own, or one its defaults leave out.
  defp covers?({{kind, name, arity}, _anno, _signature, _doc, meta}, name, wanted)
       when kind in [:function, :macro],
       do: wanted in (arity - Map.get(meta, :defaults, 0))..arity//1

  defp covers?(_entry, _name, _arity), do: false

  defp function_docs(module, shown, {_key, _anno, signatures, doc, _meta}) do
    heads = Enum.map(signatures, &"#{inspect(module)}.#{&1}")

    case doc do
      %{"en" => text} -> heads ++ ["" | text(text)]
      :hidden -> ["#{shown} is internal: it has no documentation"]
      _none -> heads ++ ["", "#{shown} has no documentation"]
    end
  end

  defp text(doc), do: doc |> String.trim_trailing() |> String.split("\n")
end
