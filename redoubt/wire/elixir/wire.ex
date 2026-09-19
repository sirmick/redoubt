# The shared part of the generated typed-message codecs (redoubt-wire's `typed` module, in
# Elixir). Hand-written; the per-protocol modules in proto/ are generated and call these.
defmodule Redoubt.Wire do
  @moduledoc """
  Typed-message framing (planning/redoubt/WIRE.md), the Elixir twin of redoubt-wire's
  `typed` module; the two are held to the same test vectors.

  A message is four words (non-negative integers), a buffer (a binary, `<<>>` if none) and a
  handle count. Word 0 of a request is its opcode; word 0 of a reply is a status, 0 for
  success, otherwise an error code (and then words 1..3 are zero, there are no handles and
  the buffer is ignored). An **inline** message packs its fields into words 1..3, four bytes
  per word, little-endian, zero-padded (12 bytes: the capacity of three 32-bit words, so the
  layout is the same on rv32 and rv64). A **buffer** message puts its fields in the buffer
  and their length in word 1; words 2 and 3 are zero. A request written into a 9P file is its
  opcode as a 32-bit little-endian integer followed by the buffer-shape encoding. Fields use
  9P's encoding: little-endian integers, strings as a 16-bit length and UTF-8, byte arrays as
  a 32-bit length and the bytes. Every word must fit in 32 bits.

  Errors, and the Rust `codec::Error` variant each corresponds to:

  | Elixir | Rust | Meaning |
  | --- | --- | --- |
  | `:bad_message` | (a type error in Rust) | not `{name, map}`, an unknown name, or the wrong keys |
  | `:bad_value` | (a type error in Rust) | an integer out of range, a string not UTF-8 or too long |
  | `:too_large` | `TooLarge` | the encoding exceeds 64 KiB (or 12 bytes inline) |
  | `:bad_words` | `BadWords` | not four integers, one beyond 32 bits, or an unused word not zero |
  | `:bad_opcode` | `BadOpcode` | an opcode the protocol does not define (or without a reply) |
  | `:bad_handles` | `BadHandles` | a handle count other than the layout's |
  | `:unexpected_buffer` | `UnexpectedBuffer` | an inline message with a buffer |
  | `:short` | `Short` | word 1 or a file names more bytes than there are |
  | `:short_fields` | `Short` | the fields end before their layout does |
  | `:bad_utf8` | `BadUtf8` | a string field that is not UTF-8 |
  | `:trailing` | `Trailing` | bytes after the fields, or nonzero inline padding |
  | `:bad_status` | `BadStatus` | an error reply with a code not in the protocol's table |
  """

  import Bitwise

  @msize 65536
  @inline_bytes 12
  @max_u32 0xFFFFFFFF

  ## Encoding

  @doc "Encodes a request: `enc` returns `{opcode, iodata}` or throws `{:wire, reason}`."
  def encode(message, layouts, enc), do: encoding(message, fn op, io -> frame(op, layouts, op, io) end, enc)

  @doc "Encodes a successful reply: word 0 is 0."
  def encode_reply(reply, layouts, enc), do: encoding(reply, fn op, io -> frame(0, layouts, op, io) end, enc)

  @doc "The words of the error reply `name`."
  def encode_error(name, errors) do
    case Enum.find(errors, fn {_code, n} -> n == name end) do
      {code, _} -> {:ok, [code, 0, 0, 0], <<>>}
      nil -> {:error, :bad_message}
    end
  end

  @doc "Encodes a request in the file framing: `{:ok, bytes}`."
  def encode_file(message, layouts, enc) do
    encoding(message, fn op, io ->
      {_name, _shape, handles} = Map.fetch!(layouts, op)
      if handles != 0, do: throw({:wire, :bad_handles})
      bytes = IO.iodata_to_binary([<<op::little-32>>, io])
      if byte_size(bytes) > @msize, do: throw({:wire, :too_large})
      {:ok, bytes}
    end, enc)
  end

  defp encoding({name, fields}, frame, enc) when is_atom(name) and is_map(fields) do
    {op, io} = enc.(name, fields)
    frame.(op, io)
  catch
    {:wire, reason} -> {:error, reason}
  end

  defp encoding(_, _, _), do: {:error, :bad_message}

  defp frame(word0, layouts, op, io) do
    body = IO.iodata_to_binary(io)

    case Map.fetch!(layouts, op) do
      {_name, :inline, _handles} ->
        # The generator only makes a message inline if it fits; this guards against a bug there.
        if byte_size(body) > @inline_bytes, do: throw({:wire, :too_large})
        pad = @inline_bytes - byte_size(body)
        <<w1::little-32, w2::little-32, w3::little-32>> = <<body::binary, 0::size(pad * 8)>>
        {:ok, [word0, w1, w2, w3], <<>>}

      {_name, :buffer, _handles} ->
        if byte_size(body) > @msize, do: throw({:wire, :too_large})
        {:ok, [word0, byte_size(body), 0, 0], body}
    end
  end

  @doc "An unsigned integer of `bits` bits, little-endian; throws if out of range."
  def u(v, bits) when is_integer(v) and v >= 0 and v < 1 <<< bits, do: <<v::little-size(bits)>>
  def u(_, _), do: throw({:wire, :bad_value})

  @doc "A string: 16-bit length and UTF-8; throws if not UTF-8 or too long."
  def str(v) when is_binary(v) and byte_size(v) <= 0xFFFF do
    if String.valid?(v), do: [<<byte_size(v)::little-16>>, v], else: throw({:wire, :bad_value})
  end

  def str(_), do: throw({:wire, :bad_value})

  @doc "A byte array: 32-bit length and the bytes."
  def bytes(v) when is_binary(v) and byte_size(v) <= @max_u32, do: [<<byte_size(v)::little-32>>, v]
  def bytes(_), do: throw({:wire, :bad_value})

  ## Decoding

  @doc "Decodes a request: `read` parses the fields of an opcode from a binary."
  def decode(words, buffer, handles, layouts, read) when is_binary(buffer) and is_integer(handles) do
    with {:ok, op} <- word0(words),
         {:ok, layout} <- lookup(layouts, op) do
      fields(op, layout, words, buffer, handles, read)
    end
  end

  def decode(_, _, _, _, _), do: {:error, :bad_message}

  @doc "Decodes the reply to the request `op`: `{:ok, reply}`, `{:failed, error}` or `{:error, reason}`."
  def decode_reply(op, words, buffer, handles, layouts, errors, read)
      when is_integer(op) and is_binary(buffer) and is_integer(handles) do
    with {:ok, layout} <- lookup(layouts, op),
         {:ok, status} <- word0(words) do
      cond do
        status == 0 -> fields(op, layout, words, buffer, handles, read)
        tl(words) != [0, 0, 0] -> {:error, :bad_words}
        handles != 0 -> {:error, :bad_handles}
        Map.has_key?(errors, status) -> {:failed, Map.fetch!(errors, status)}
        true -> {:error, :bad_status}
      end
    end
  end

  def decode_reply(_, _, _, _, _, _, _), do: {:error, :bad_message}

  @doc "Decodes a request written into a 9P file."
  def decode_file(bytes, _layouts, _read) when is_binary(bytes) and byte_size(bytes) > @msize,
    do: {:error, :too_large}

  def decode_file(<<op::little-32, rest::binary>>, layouts, read) do
    with {:ok, {_name, _shape, count}} <- lookup(layouts, op),
         :ok <- check_handles(count, 0),
         {:ok, name, fields, left} <- read.(op, rest),
         :ok <- finish(:buffer, left) do
      {:ok, {name, fields}}
    end
  end

  def decode_file(bytes, _, _) when is_binary(bytes), do: {:error, :short}
  def decode_file(_, _, _), do: {:error, :bad_message}

  @doc "Wraps a `read` result: every string field must be UTF-8."
  def utf8(strings, result) do
    if Enum.all?(strings, &String.valid?/1), do: result, else: {:error, :bad_utf8}
  end

  defp fields(op, {_name, shape, count}, words, buffer, handles, read) do
    with :ok <- check_handles(handles, count),
         {:ok, body} <- body(shape, words, buffer),
         {:ok, name, fields, rest} <- read.(op, body),
         :ok <- finish(shape, rest) do
      {:ok, {name, fields}}
    end
  end

  # Word 0 of four non-negative integers, within 32 bits.
  defp word0([w0, w1, w2, w3] = words)
       when is_integer(w0) and w0 >= 0 and w0 <= @max_u32 and is_integer(w1) and w1 >= 0 and
              is_integer(w2) and w2 >= 0 and is_integer(w3) and w3 >= 0,
       do: {:ok, hd(words)}

  defp word0(_), do: {:error, :bad_words}

  defp lookup(layouts, op) do
    case Map.fetch(layouts, op) do
      {:ok, layout} -> {:ok, layout}
      :error -> {:error, :bad_opcode}
    end
  end

  defp check_handles(count, count), do: :ok
  defp check_handles(_, _), do: {:error, :bad_handles}

  # The 12 inline bytes (an inline message has no buffer), or a buffer message's first
  # word-1 bytes.
  defp body(:inline, _words, buffer) when buffer != <<>>, do: {:error, :unexpected_buffer}

  defp body(:inline, [_, w1, w2, w3], _buffer)
       when w1 <= @max_u32 and w2 <= @max_u32 and w3 <= @max_u32,
       do: {:ok, <<w1::little-32, w2::little-32, w3::little-32>>}

  defp body(:buffer, [_, len, 0, 0], buffer) when len <= @msize do
    if len <= byte_size(buffer), do: {:ok, binary_part(buffer, 0, len)}, else: {:error, :short}
  end

  defp body(_, _, _), do: {:error, :bad_words}

  defp finish(:inline, rest) do
    if rest == :binary.copy(<<0>>, byte_size(rest)), do: :ok, else: {:error, :trailing}
  end

  defp finish(:buffer, <<>>), do: :ok
  defp finish(:buffer, _), do: {:error, :trailing}
end
