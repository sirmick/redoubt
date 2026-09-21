%% A module no compiler would produce: spin/0 loops on a bare jump, with no call to count as a
%% reduction. Used to check that such code is still preempted. Build: erlc +from_asm jumploop.S
{module, jumploop}.
{exports, [{done,0},{spin,0}]}.
{attributes, []}.
{labels, 6}.

{function, spin, 0, 2}.
  {label,1}.
    {func_info,{atom,jumploop},{atom,spin},0}.
  {label,2}.
    {jump,{f,2}}.

{function, done, 0, 4}.
  {label,3}.
    {func_info,{atom,jumploop},{atom,done},0}.
  {label,4}.
    {move,{integer,42},{x,0}}.
    return.
