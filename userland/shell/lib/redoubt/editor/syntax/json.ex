defmodule Redoubt.Editor.Syntax.Json do
  @moduledoc "JSON, for the editor's highlighting (`Redoubt.Editor.Syntax`): `.json`."

  @behaviour Redoubt.Editor.Syntax

  alias Redoubt.Editor.Syntax

  @table Syntax.table(
           strings: [{~S|"|, ~S|"|, :string, false, true}],
           constants: ~w(true false null)
         )

  @impl Syntax
  def line(text, state), do: Syntax.scan(@table, text, state)
end
