import {useEffect,useRef,type ReactNode} from 'react';
export function Modal({children,onClose,label="Review and edit"}:{children:ReactNode;onClose:()=>void;label?:string}){const ref=useRef<HTMLDialogElement>(null);useEffect(()=>{ref.current?.showModal();},[]);return <dialog ref={ref} aria-label={label} onCancel={onClose} onClose={onClose}>{children}</dialog>;}
