-- Timers, tasks and tweens: after / every, a coroutine task with wait(), and eased movement.
local box, log = nil, {}

function start()
    sun(vec3(-0.4, -1, -0.3), "white", 1.2)
    spawn(Mesh.plane(20), "#33404f")
    box = spawn(Mesh.cube(), "#ff9933", { position = vec3(-3, 0.5, 0) })
    camera.position = vec3(0, 4, 9)
    camera:look_at(vec3(0, 0.5, 0))
    every(1, function(n) table.insert(log, "tick " .. n) if #log > 4 then table.remove(log, 1) end end)
    task(function()
        while true do
            tween(box, { position = vec3(3, 0.5, 0) }, 1.2, "in_out")
            wait(1.4)
            tween(box, { position = vec3(-3, 0.5, 0) }, 1.2, "out_elastic")
            wait(1.6)
        end
    end)
end

function render()
    for i, line in log do draw.text(line, 16, 16 + (i - 1) * 26, { size = 20, shadow = true }) end
end