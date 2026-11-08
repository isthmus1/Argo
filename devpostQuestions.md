## Inspiration
AI continues and will continue to become increasingly smarter and cheaper, and thus finding complex vulnerabilities will become cheaper and faster. In this new age of cybersecurity, memory safety will become more important than ever before, and we will need tools to modernize and patch codebases as fast as new tools uncover vulnerabilities. Rust makes it a priority for your code to be memory safe {add here}.

## What it does
Argo parses a legacy C codebase into a Blueprint-style graph and converts one node at a time into rust, interoperating the new rust code with the rest of the C codebase and running tests every step of the way. 
We first parse the whole C codebase into an AST, which we then load into spaceitmeDB and visualize using {library}. We then deploy agents into their own atomic copy of the codebase, where they convert c code into rust and recompile and test everything. Then we merge everything into a rust database, and we continue with the next bit of code.
For our demo we have chosen to translate TinyEMU's C code into rust.

## How we built it
We {recognized} our different strengths and weaknesses to create different roles for each member to fill. Most of our contributions came from {one person} using a hill climbing method in order to orchestrate a pannel of coding agents and a pannel of reviewing agents. {spec file, benefits of clean environment}. At the same time, {another person} handled making functions for calling nemotron compatible with the planned architecture of tha application. {final person} built a rust compiler library that made it possible for the spacetimeDB rust database to be compiled alongside the C database.

## Challenges we ran into
One of the first challenges we ran into was how to properly separate our code into {technical term}. //elaborate
Second challenege we faced was how to properly compile the combined rust and c codebase. //Elaborate

## Accomplishments that we're proud of
We're quite proud we were able to convert a full CPU emulator into rust code without any human intervention. The whole idea at the beginning seemed like a stretch to be able to do without any intervention, especially choosing such an amitious codebase as a demo. {larp}
The Blueprint-style graph UI is quite pleasing to look at, and we spent a lot of time making sure the whole frontend experience was smooth; we're quite proud of how it turned out.
Compiling from spacetimeDB {expand}

## What we learned
Hill-climbing?? or at least integrating that into a team environment. Spacetime db.

## What's next for Argo

(tbd)
