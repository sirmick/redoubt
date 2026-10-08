defmodule Redoubt.Editor.Syntax.Ex do
  @moduledoc "Elixir, for the editor's highlighting (`Redoubt.Editor.Syntax`): `.ex` and `.exs`."

  @behaviour Redoubt.Editor.Syntax

  alias Redoubt.Editor.Syntax

  @table Syntax.table(
           comment: ["#"],
           strings: [
             {~S|"""|, ~S|"""|, :string, true, true},
             {"'''", "'''", :string, true, true},
             {~S|"|, ~S|"|, :string, true, true},
             {"'", "'", :string, false, true}
           ],
           keywords: ~w(do end fn when and or not in if else unless case cond with for try catch
             rescue after receive def defp defmacro defmacrop defmodule defstruct defprotocol
             defimpl defdelegate defguard defguardp defexception defoverridable quote unquote
             import alias require use raise reraise throw),
           constants: ~w(true false nil),
           capitals: :constant,
           atoms: true,
           char: "?"
         )

  @impl Syntax
  def line(text, state), do: Syntax.scan(@table, text, state)
end
