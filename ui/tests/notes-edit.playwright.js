// Edit in place (SPEC 7.11, DESIGN 4.9) against a browser-only IPC fixture; save_notes itself has Rust tests.
async (page) => {
  const results=[];
  for(const theme of ['light','dark']) {
    const p=await page.context().newPage();
    const errors=[];
    p.on('pageerror',e=>errors.push(e.message));
    await p.emulateMedia({colorScheme:theme});
    await p.setViewportSize({width:1046,height:662});
    await p.addInitScript(()=>{
      Object.defineProperty(navigator,'platform',{value:'MacIntel'});
      const summary={id:'123',title:'FCM vs Pusher Beams',created_ms:Date.now(),status:'Ready',marks_ms:[],duration_ms:87000,notes_ms:48000,spellings:[]};
      const meeting={summary,notes_md:'### Action items\n- [ ] Kevin: Check whether the 2021 Firebase project is still active [00:56]\n- [ ] You: Share the device table design [01:20]\n\n### Overview\n- Compared **FCM** with push beams for Android push in the IGB app.\n- Decided to **keep FCM** and build device management ourselves.\n\n### Cost\n- **FCM has no messaging fee.** [01:00]\n- See [the pricing page](https://example.com) later.\n\n<details><summary>Transcript</summary>\n\n[00:16] Me: hi\n\n</details>\n',transcript:'[00:16] Me: Pusher beams relies on FCM.\n[00:56] Them: Check the Firebase project.\n[01:00] Me: FCM has no messaging fee.\n[01:20] Me: I will share the table.',transcript_heard:''};
      const tracks=[{path:'/fixture0.wav',name:'mic.wav',speaker:'You',offset_ms:0,duration_ms:87000,bytes:2100000,waveform:Array(200).fill(.3)}];
      let next=0;window.testCalls=[];window.meeting=meeting;
      window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:()=>{}};
      window.__TAURI_INTERNALS__={convertFileSrc:path=>path,transformCallback:()=>++next,unregisterCallback:()=>{},invoke:async(command,args)=>{
        window.testCalls.push({command,args});
        if(command==='plugin:event|listen')return next;
        if(command==='get_state')return {state:{type:'Idle'},meeting_id:null,companion:{visible:false,size:'Pill'},elapsed_ms:0,quit_pending:false};
        if(command==='notification_permission')return true;
        if(command==='list_meetings')return [summary];
        if(command==='deleted_meetings')return [];
        if(command==='get_meeting')return meeting;
        if(command==='meeting_media')return {tracks,duration_ms:87000,folder:'~/Documents/Motetto/123'};
        if(command==='save_notes'){ if(summary.locked)throw Error('These notes are locked. Unlock them to edit.'); if(args.expected!==meeting.notes_md)throw Error('Stale notes'); meeting.notes_md=args.markdown; return args.markdown; }
        if(command==='set_locked'){ summary.locked=args.locked; return {...summary}; }
        // As transcript_edits.rs: only the text after "[mm:ss] Speaker: " changes; heard text kept for Revert.
        if(command==='save_transcript_line'){
          if(summary.locked)throw Error('These notes are locked. Unlock them to edit.');
          const heard=meeting.transcript_heard||meeting.transcript, lines=meeting.transcript.split('\n');
          const i=lines.map((l,k)=>[l,k]).filter(([l])=>l.trim())[args.index][1];
          lines[i]=/^\[[^\]]*\] [^:]*: /.exec(lines[i])[0]+args.text; meeting.transcript=lines.join('\n');
          meeting.transcript_heard=meeting.transcript===heard?'':heard; return meeting.transcript;
        }
        return false;
      }};
    });
    await p.goto('http://127.0.0.1:3000/main/');
    await p.getByRole('button',{name:/FCM vs Pusher Beams/}).first().click();
    await p.locator('.notes [data-lines]').first().waitFor();
    const failures=[];
    const saved=()=>p.evaluate(()=>window.meeting.notes_md);
    const geo=sel=>p.locator(sel).first().evaluate(e=>{const r=e.getBoundingClientRect(),s=e.closest('.docbody')||document.scrollingElement;return [Math.round(r.top),Math.round(r.height),s.scrollTop].join(',');});
    const endOf=sel=>p.locator(sel).first().evaluate(e=>{const w=document.createTreeWalker(e,NodeFilter.SHOW_TEXT);let n,last;while((n=w.nextNode()))if(!n.parentElement.closest('button,.owner,input')&&n.nodeValue.replace(/\u200b/g,'').trim())last=n;const i=last.nodeValue.trimEnd().length,r=document.createRange();r.setStart(last,i-1);r.setEnd(last,i);const b=r.getBoundingClientRect();return {x:b.right-1,y:b.y+b.height/2};});
    const overview='.notes li[data-lines]:has-text("Compared")';

    // 1. No reflow: focus and blur leave the block where it was; the edited block shows one hairline.
    const g0=await geo(overview);
    let at=await endOf(overview); await p.mouse.click(at.x,at.y);
    const g1=await geo(overview);
    const look=await p.locator(overview).evaluate(e=>({editing:e.classList.contains('editing'),ce:e.contentEditable,bg:getComputedStyle(e).backgroundImage,outline:getComputedStyle(e).outlineStyle}));
    await p.mouse.click(1000,640); await p.waitForTimeout(200);
    const g2=await geo(overview);
    if(g0!==g1||g1!==g2)failures.push(`reflow: ${g0} | ${g1} | ${g2}`);
    if(!look.editing||look.ce!=='true')failures.push('click did not start editing');
    if(!look.bg.includes('gradient'))failures.push('no hairline while editing');
    if(look.outline!=='none')failures.push('edited block draws an outline');
    if(await p.locator('.notes').evaluate(e=>getComputedStyle(e).outlineStyle)!=='none')failures.push('notes container focus ring');

    // 2. Typing saves only the typed text; bold, bullet and line range stay.
    at=await endOf(overview); await p.mouse.click(at.x,at.y);
    await p.keyboard.type(' Today');
    await p.keyboard.press('Escape'); await p.waitForTimeout(200);
    if(!(await saved()).includes('- Compared **FCM** with push beams for Android push in the IGB app. Today\n'))failures.push('overview edit not saved as Markdown');

    // 3. Note syntax converts as typed; the owner and checkbox are kept.
    const task='.todo li:has-text("Firebase")';
    at=await endOf(task+' .task-text'); await p.mouse.click(at.x,at.y);
    await p.keyboard.type(' and [00:16] **soon**');
    const live=await p.locator(task).evaluate(e=>({chips:[...e.querySelectorAll('.src')].map(c=>c.textContent),bold:[...e.querySelectorAll('.task-text b')].map(b=>b.textContent)}));
    if(!live.chips.includes('00:16')||!live.bold.includes('soon'))failures.push(`live syntax: ${JSON.stringify(live)}`);
    await p.keyboard.press('Escape'); await p.waitForTimeout(200);
    const taskLine=(await saved()).split('\n')[1];
    if(!/^- \[ \] Kevin: Check whether the 2021 Firebase project is still active( \[00:56\])? and \[00:16\] \*\*soon\*\*( \[00:56\])?$/.test(taskLine))failures.push(`task line: ${taskLine}`);
    if(await p.locator(task+' .src',{hasText:'00:16'}).count()!==1)failures.push('typed time is not a chip after saving');

    // 4. Enter saves and leaves the block; ⇧Enter stays in it; ⌘Enter splits a task into a new
    // unticked task; ⌘Z (no caret) undoes it.
    const blocksBefore=await p.locator('.notes [data-lines]').count();
    at=await endOf('.todo li:has-text("device table") .task-text'); await p.mouse.click(at.x,at.y);
    await p.keyboard.type(' now'); await p.keyboard.press('Shift+Enter'); await p.keyboard.type('please');
    if(!await p.locator('.notes .editing').count())failures.push('Shift+Enter left the block');
    await p.keyboard.press('Enter'); await p.waitForTimeout(250);
    if(await p.locator('.notes .editing').count())failures.push('Enter did not leave the block');
    if(await p.locator('.notes [data-lines]').count()!==blocksBefore)failures.push('Enter added a block');
    if(!/- \[ \] You: Share the device table design( \[01:20\])? now please( \[01:20\])?\n/.test(await saved()))failures.push(`Enter did not save one line: ${(await saved()).split('\n')[2]}`);
    at=await endOf('.todo li:has-text("device table") .task-text'); await p.mouse.click(at.x,at.y);
    await p.keyboard.press('Meta+Enter'); await p.waitForTimeout(250);
    await p.keyboard.type('Book the review');
    await p.keyboard.press('Escape'); await p.waitForTimeout(250);
    if(!(await saved()).includes('\n- [ ] Book the review\n'))failures.push('Cmd+Enter did not make a new task');
    await p.keyboard.press('Meta+z'); await p.waitForTimeout(250);
    if((await saved()).includes('Book the review'))failures.push('undo did not remove the new task');

    // 5. Paste is plain text; a block with a link stays read-only; the transcript copy is untouched.
    at=await endOf(overview); await p.mouse.click(at.x,at.y);
    await p.evaluate(()=>{const d=new DataTransfer();d.setData('text/html','<img src=x onerror=alert(1)><b>bold</b>');d.setData('text/plain','pasted text');document.activeElement.dispatchEvent(new ClipboardEvent('paste',{clipboardData:d,bubbles:true,cancelable:true}));});
    await p.keyboard.press('Escape'); await p.waitForTimeout(200);
    const md=await saved();
    if(!md.includes('Todaypasted text')&&!md.includes('Today pasted text'))failures.push('plain paste missing');
    if(md.includes('<img')||md.includes('<b>'))failures.push('pasted HTML reached the file');
    if(await p.locator('.notes li:has-text("pricing page")').getAttribute('data-editable')!=='0')failures.push('link block is editable');
    if(!md.includes('<details><summary>Transcript</summary>\n\n[00:16] Me: hi\n\n</details>'))failures.push('transcript copy changed');

    // 6. Lock notes: read-only for review; ticking still works; the Locked mark unlocks.
    await p.getByRole('button',{name:'More actions'}).click();
    await p.getByRole('menuitemcheckbox',{name:'Lock notes'}).click();
    await p.locator('.locked-mark').waitFor();
    const before=await saved();
    at=await endOf(overview); await p.mouse.click(at.x,at.y); await p.keyboard.type('NOPE'); await p.mouse.click(1000,640); await p.waitForTimeout(200);
    if(await p.locator('.notes .editing').count()||(await saved())!==before)failures.push('locked notes were edited');
    if(await p.locator('.notes [data-editable="1"]').count())failures.push('locked block still marked editable');
    if(await p.locator('.dochead .pen').count())failures.push('rename pencil shown while locked');
    if(await p.locator('.notes .notes-hint').innerText()!=='Locked for review. Click Locked to edit again.')failures.push('locked hint missing');
    await p.getByRole('button',{name:'More actions'}).click();
    if(!await p.getByRole('menuitem',{name:'Write the notes again'}).isDisabled())failures.push('regenerate allowed while locked');
    if(!await p.getByRole('menuitem',{name:'Rename'}).isDisabled())failures.push('rename allowed while locked');
    await p.keyboard.press('Escape');
    await p.screenshot({path:`.playwright-out/notes-locked-${theme}.png`});
    await p.locator('.locked-mark').click(); await p.waitForTimeout(200);
    if(await p.locator('.locked-mark').count())failures.push('Locked mark did not unlock');
    at=await endOf(overview); await p.mouse.click(at.x,at.y);
    if(!await p.locator('.notes .editing').count())failures.push('not editable after unlock');
    await p.keyboard.press('Escape');

    // 6b. The @ picker is the one way to assign: You first, then people already owning tasks.
    at=await endOf('.todo li:has-text("device table") .task-text'); await p.mouse.click(at.x,at.y);
    await p.keyboard.type(' @Ke');
    const pickNames=await p.locator('.mention [role=option]').allInnerTexts();
    if(pickNames.length!==1||pickNames[0]?.trim()!=='Kevin')failures.push(`picker options (new only when nothing matches): ${JSON.stringify(pickNames)}`);
    await p.keyboard.press('Enter'); await p.waitForTimeout(250);
    if(!(await saved()).includes('\n- [ ] Kevin: Share the device table design now please [01:20]\n'))failures.push(`assign by picker: ${(await saved()).split('\n')[2]}`);
    if(await p.locator('.todo li:has-text("device table") .owner').innerText()!=='Kevin')failures.push('owner pill not updated');
    // A name nobody has yet; picking on a bullet makes it a task.
    at=await endOf(overview); await p.mouse.click(at.x,at.y);
    await p.keyboard.type(' @Linh Tran');
    const fresh=await p.locator('.mention [role=option][aria-selected="true"]').innerText();
    if(!fresh.includes('Linh Tran')||!fresh.includes('new'))failures.push(`new name option: ${fresh}`);
    await p.keyboard.press('Enter'); await p.waitForTimeout(250);
    if(!/\n- \[ \] Linh Tran: Compared \*\*FCM\*\* with push beams/.test(await saved()))failures.push('bullet not turned into an assigned task');
    if(await p.locator('.mention').count())failures.push('picker still open');
    // The owner pill's menu: show only one person's items, then fix a misheard name everywhere.
    await p.locator('.todo li:has-text("device table") button.owner').click();
    await p.getByRole('menuitem',{name:"Show only Kevin's items"}).click();
    const shown=await p.locator('.todo li').evaluateAll(ls=>ls.map(l=>l.querySelector('.owner')?.textContent));
    if(!shown.length||shown.some(n=>n!=='Kevin'))failures.push(`owner filter: ${JSON.stringify(shown)}`);
    await p.getByRole('button',{name:'Show all',exact:true}).click();
    await p.locator('.todo li:has-text("device table") button.owner').click();
    await p.getByRole('menuitem',{name:'Fix this name…'}).click();
    // The name box shows the whole name, selected, with one edge (a 15 px checkbox rule once squeezed it).
    const box=await p.locator('input.ownerin').evaluate(i=>({fits:i.scrollWidth<=i.clientWidth&&i.offsetWidth>30,value:i.value,selected:i.selectionEnd-i.selectionStart,shadow:getComputedStyle(i).boxShadow}));
    if(!box.fits||box.value!=='Kevin'||box.selected!==5||(box.shadow.match(/inset/g)||[]).length!==1)failures.push(`name box: ${JSON.stringify(box)}`);
    await p.keyboard.press('Meta+a'); await p.keyboard.type('Kevan'); await p.keyboard.press('Enter'); await p.waitForTimeout(250);
    const fixed=await saved();
    if((fixed.match(/Kevan: /g)||[]).length!==2||fixed.includes('Kevin: '))failures.push('Fix this name did not rename every Kevin task');
    // A different person only reassigns that task.
    await p.locator('.todo li:has-text("device table") button.owner').click();
    await p.getByRole('menuitem',{name:'Fix this name…'}).click();
    await p.keyboard.press('Meta+a'); await p.keyboard.type('Linh Tran'); await p.keyboard.press('Enter'); await p.waitForTimeout(250);
    const re=await saved();
    if(!re.includes('- [ ] Linh Tran: Share the device table design')||!re.includes('Kevan: Check whether'))failures.push('reassign changed more than one task');
    // Esc closes the picker and keeps the typed text.
    at=await endOf('.notes li[data-lines]:has-text("Decided")'); await p.mouse.click(at.x,at.y);
    await p.keyboard.type(' @x'); await p.keyboard.press('Escape');
    if(await p.locator('.mention').count())failures.push('Esc did not close the picker');
    await p.keyboard.press('Escape'); await p.waitForTimeout(200);

    // 7. Transcript: only the words are edited, on one hairline in the speaker's colour; no reflow.
    await p.getByRole('tab',{name:'Transcript',exact:true}).click();
    const row=i=>`.tline[data-line="${i}"]`;
    const tg0=await geo(row(0));
    at=await endOf(row(0)+' .words'); await p.mouse.click(at.x,at.y);
    const tlook=await p.locator(row(0)+' .words').evaluate(e=>({ce:e.contentEditable,bg:getComputedStyle(e).backgroundImage,active:document.activeElement===e}));
    if(!tlook.active||!['plaintext-only','true'].includes(tlook.ce))failures.push(`transcript line not editable: ${JSON.stringify(tlook)}`);
    if(!tlook.bg.includes('gradient'))failures.push('no hairline under the edited line');
    if(await geo(row(0))!==tg0)failures.push('transcript line reflowed on focus');
    await p.keyboard.type(' today');
    await p.keyboard.press('Enter'); await p.waitForTimeout(250);
    const tr=await p.evaluate(()=>window.meeting);
    if(tr.transcript.split('\n')[0]!=='[00:16] Me: Pusher beams relies on FCM. today')failures.push(`line not saved: ${tr.transcript.split('\n')[0]}`);
    if(tr.transcript_heard.split('\n')[0]!=='[00:16] Me: Pusher beams relies on FCM.')failures.push('heard text lost');
    if(await p.evaluate(()=>document.activeElement?.closest('.tline')?.dataset.line)!=='1')failures.push('Enter did not move to the next line');
    await p.keyboard.press('Escape'); await p.waitForTimeout(150);
    // 8. The edited dot shows what was heard; Revert restores it.
    const dot=await p.locator(row(0)+' .edot').boundingBox();
    if(!dot)failures.push('no edited dot');
    else {
      await p.mouse.move(dot.x+7,dot.y+7); await p.waitForTimeout(100);
      if(!(await p.locator(row(0)+' .epop').innerText()).includes('Pusher beams relies on FCM.'))failures.push('popover lacks the heard text');
      // The edited dot sits on the middle of the line's first text line (owner, 2026-10-20: it sat 3 px high).
      const mid=await p.locator(row(0)).evaluate(r=>{const w=r.querySelector('.words'),g=document.createRange();g.selectNodeContents(w);const t=g.getClientRects()[0],d=r.querySelector('.edot').getBoundingClientRect();return Math.abs((t.top+t.bottom)/2-(d.top+d.bottom)/2);});
      if(mid>1)failures.push(`edited dot ${mid.toFixed(1)} px off the text's middle`);
      const rb=await p.locator(row(0)+' .epop button').boundingBox();
      await p.mouse.move(rb.x+rb.width/2,rb.y+rb.height/2,{steps:6}); await p.mouse.down(); await p.mouse.up(); await p.waitForTimeout(250);
      if((await p.evaluate(()=>window.meeting.transcript_heard))!=='')failures.push('Revert did not restore the line');
      if(await p.locator(row(0)+' .edot').count())failures.push('edited dot after Revert');
    }
    // 9. An emptied line is removed but keeps its place, with Revert.
    at=await endOf(row(2)+' .words'); await p.mouse.click(at.x,at.y);
    await p.keyboard.press('Meta+a'); await p.keyboard.press('Backspace'); await p.keyboard.press('Escape'); await p.waitForTimeout(250);
    if(!(await p.locator(row(2)).innerText()).includes('Removed from the transcript'))failures.push('emptied line not shown as removed');
    await p.locator(row(2)+' .lnk').click(); await p.waitForTimeout(250);
    if((await p.evaluate(()=>window.meeting.transcript))!==(await p.evaluate(()=>window.meeting.transcript_heard||window.meeting.transcript)))failures.push('removed line not reverted');
    // 10. Locked: the transcript is read-only too.
    await p.getByRole('button',{name:'More actions'}).click();
    await p.getByRole('menuitemcheckbox',{name:'Lock notes'}).click();
    await p.locator('.locked-mark').waitFor();
    if(await p.locator('.tline .words[contenteditable]').count())failures.push('locked transcript is editable');
    if(await p.locator('.transcript-edit .thint').innerText()!=='Locked for review. Click Locked to edit again.')failures.push('locked transcript hint missing');
    await p.locator('.locked-mark').click(); await p.waitForTimeout(150);
    await p.screenshot({path:`.playwright-out/transcript-edit-${theme}.png`});

    await p.screenshot({path:`.playwright-out/notes-edit-${theme}.png`});
    results.push({theme,failures,errors});
    await p.close();
  }
  if(results.some(r=>r.failures.length||r.errors.length))throw Error(JSON.stringify(results));
  return results;
}
