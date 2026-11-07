const {test}=require('node:test');
const assert=require('node:assert/strict');
const cube=require('../../ui/js/cube-alert.js');
const a={sequence:1,ts_ms:1000,entity_id:1234,status:'detected',cube_type:'Synthetic cube',distance_m:null,direction:null};
test('unknown distance is omitted rather than 0m or NaN',()=>assert.deepEqual(cube.text(a),{title:'Hidden Cube nearby',subline:'Synthetic cube'}));
test('fresh distance and compass appear',()=>assert.equal(cube.text({...a,distance_m:12,direction:'NE'}).subline,'Synthetic cube \u00b7 12m NE'));
test('gone is clearly distinct',()=>assert.deepEqual(cube.text({...a,status:'gone'}),{title:'Cube no longer nearby',subline:'Synthetic cube \u00b7 Out of range'}));
test('dismiss and removal never resurrect older detected state',()=>{
 const gone={...a,sequence:2,status:'gone',ts_ms:2000};
 assert.equal(cube.select([a,gone],new Set(),2001),gone);
 assert.equal(cube.select([a,gone],new Set([cube.key(gone)]),2001),null);
 assert.equal(cube.select([a,gone],new Set(),8001),null);
});
test('detected stays selected without more packets',()=>assert.equal(cube.select([a],new Set(),600000).entity_id,1234));
test('invalid distance never renders',()=>assert.equal(cube.text({...a,distance_m:NaN,direction:'N'}).subline,'Synthetic cube'));
test('dismiss target alone captures pointer through transparent body',()=>{
 let click;const button={addEventListener:(e,f)=>{click=f;},getBoundingClientRect:()=>({left:200,right:228,top:50,bottom:78})};
 const title={},subline={}; const element={hidden:true,classList:{toggle(){}},querySelector:s=>s==='button'?button:s==='[data-cube-title]'?title:subline};
 const view=cube.create(element); view.update({cube_alerts:[a],settings:{}},1001);
 assert.equal(view.hit(210,60),true);assert.equal(view.hit(100,60),false);
 click({stopPropagation(){}});assert.equal(element.hidden,true);assert.equal(view.hit(210,60),false);
 view.update({cube_alerts:[a],settings:{}},1002);assert.equal(element.hidden,true);
});
test('disabled config hides alert',()=>{
 const b={addEventListener(){}};const el={hidden:false,querySelector:()=>b};
 cube.create(el).update({settings:{hidden_cube_alerts:false},cube_alerts:[a]},1001);assert.equal(el.hidden,true);
});

test('detected alert stays up until gone, opened or dismissed',()=>{
 let callback,delay;const timers={set:(fn,ms)=>{callback=fn;delay=ms;return 1;},clear(){}};
 const label={};const button={addEventListener(){}};
 const el={hidden:true,classList:{toggle(){}},querySelector:s=>s==='button'?button:label};
 cube.create(el,timers).update({cube_alerts:[a],settings:{}},2000);
 assert.equal(delay,undefined);assert.equal(el.hidden,false);
});
